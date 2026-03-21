use std::process::Command;

use nanobot_config::Config;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallRequest>,
    pub finish_reason: String,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider returned no response")]
    EmptyResponse,
    #[error("provider error: {0}")]
    Message(String),
    #[error("request execution failed: {0}")]
    Request(String),
    #[error("response parse failed: {0}")]
    ResponseParse(String),
}

pub trait LlmProvider: Send + Sync {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError>;
    fn default_model(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub trait HttpExecutor: Send + Sync {
    fn execute(&self, request: &HttpRequest) -> Result<String, ProviderError>;
}

#[derive(Debug, Clone, Default)]
pub struct CurlExecutor;

impl HttpExecutor for CurlExecutor {
    fn execute(&self, request: &HttpRequest) -> Result<String, ProviderError> {
        let mut command = Command::new("curl");
        command.arg("--silent").arg("--show-error");
        command.arg("-X").arg(&request.method);
        for (name, value) in &request.headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        command.arg("--data").arg(&request.body);
        command.arg(&request.url);

        let output = command
            .output()
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        if !output.status.success() {
            return Err(ProviderError::Request(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[derive(Debug, Clone)]
pub struct StaticProvider {
    model: String,
    response_prefix: String,
}

impl StaticProvider {
    pub fn new(model: impl Into<String>, response_prefix: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            response_prefix: response_prefix.into(),
        }
    }
}

impl Default for StaticProvider {
    fn default() -> Self {
        Self::new("offline/echo", "echo")
    }
}

impl LlmProvider for StaticProvider {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .ok_or(ProviderError::EmptyResponse)?;
        Ok(LlmResponse {
            content: Some(format!("{}: {}", self.response_prefix, last_user.content)),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        })
    }

    fn default_model(&self) -> &str {
        &self.model
    }
}

#[derive(Debug, Clone, Default)]
pub struct DemoToolCallingProvider;

impl LlmProvider for DemoToolCallingProvider {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.to_ascii_lowercase())
            .unwrap_or_default();

        let last_tool = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "tool")
            .map(|message| message.content.clone());

        if let Some(tool_result) = last_tool {
            return Ok(LlmResponse {
                content: Some(format!("demo complete: {tool_result}")),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            });
        }

        if last_user.contains("write") {
            return Ok(LlmResponse {
                content: None,
                tool_calls: vec![ToolCallRequest {
                    id: "fs-write-1".to_string(),
                    name: "filesystem".to_string(),
                    arguments: serde_json::json!({
                        "action": "write",
                        "path": "demo/generated.txt",
                        "content": "written by demo tool provider"
                    }),
                }],
                finish_reason: "tool_calls".to_string(),
            });
        }

        if last_user.contains("read") {
            return Ok(LlmResponse {
                content: None,
                tool_calls: vec![ToolCallRequest {
                    id: "fs-read-1".to_string(),
                    name: "filesystem".to_string(),
                    arguments: serde_json::json!({
                        "action": "read",
                        "path": "demo/generated.txt"
                    }),
                }],
                finish_reason: "tool_calls".to_string(),
            });
        }

        if last_user.contains("send") || last_user.contains("message") {
            return Ok(LlmResponse {
                content: None,
                tool_calls: vec![ToolCallRequest {
                    id: "msg-1".to_string(),
                    name: "message".to_string(),
                    arguments: serde_json::json!({
                        "content": "demo outbound message"
                    }),
                }],
                finish_reason: "tool_calls".to_string(),
            });
        }

        Ok(LlmResponse {
            content: Some("demo idle".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        })
    }

    fn default_model(&self) -> &str {
        "offline/tool-calling-demo"
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider<E = CurlExecutor> {
    api_key: String,
    api_base: String,
    model: String,
    executor: E,
}

impl<E> OpenAiCompatibleProvider<E> {
    pub fn new(
        api_key: impl Into<String>,
        api_base: impl Into<String>,
        model: impl Into<String>,
        executor: E,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_base: api_base.into(),
            model: model.into(),
            executor,
        }
    }

    pub fn build_request(&self, request: &ChatRequest) -> HttpRequest {
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": request.messages,
            "tools": request.tools.iter().map(|name| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": format!("Tool {name}")
                    }
                })
            }).collect::<Vec<_>>(),
        });
        HttpRequest {
            method: "POST".to_string(),
            url,
            headers: vec![
                (
                    "Authorization".to_string(),
                    format!("Bearer {}", self.api_key),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: body.to_string(),
        }
    }

    fn parse_response(&self, raw: &str) -> Result<LlmResponse, ProviderError> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| ProviderError::ResponseParse(error.to_string()))?;
        let choice = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .ok_or_else(|| ProviderError::ResponseParse("missing choices[0]".to_string()))?;
        let message = choice
            .get("message")
            .ok_or_else(|| ProviderError::ResponseParse("missing message".to_string()))?;
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| ToolCallRequest {
                        id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: item
                            .get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        arguments: item
                            .get("function")
                            .and_then(|function| function.get("arguments"))
                            .and_then(Value::as_str)
                            .and_then(|raw| serde_json::from_str(raw).ok())
                            .unwrap_or_else(|| serde_json::json!({})),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .unwrap_or("stop")
            .to_string();

        Ok(LlmResponse {
            content,
            tool_calls,
            finish_reason,
        })
    }
}

impl<E: HttpExecutor> LlmProvider for OpenAiCompatibleProvider<E> {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
        let http_request = self.build_request(&request);
        let raw = self.executor.execute(&http_request)?;
        self.parse_response(&raw)
    }

    fn default_model(&self) -> &str {
        &self.model
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    OpenRouter,
    OpenAI,
    Anthropic,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSelection {
    pub kind: ProviderKind,
    pub model: String,
}

impl ProviderSelection {
    pub fn detect(
        requested_model: Option<&str>,
        api_key: Option<&str>,
        api_base: Option<&str>,
    ) -> Self {
        let model = requested_model.unwrap_or("offline/echo");
        let lower_model = model.to_ascii_lowercase();
        let lower_base = api_base.unwrap_or_default().to_ascii_lowercase();

        if api_key.unwrap_or_default().starts_with("sk-or-") || lower_base.contains("openrouter") {
            return Self {
                kind: ProviderKind::OpenRouter,
                model: "openrouter/auto".to_string(),
            };
        }

        if lower_model.contains("claude") || lower_model.contains("anthropic") {
            return Self {
                kind: ProviderKind::Anthropic,
                model: model.to_string(),
            };
        }

        if lower_model.contains("gpt") || lower_model.contains("openai") {
            return Self {
                kind: ProviderKind::OpenAI,
                model: model.to_string(),
            };
        }

        Self {
            kind: ProviderKind::Offline,
            model: model.to_string(),
        }
    }
}

pub fn build_provider_from_config(config: &Config) -> Box<dyn LlmProvider> {
    let requested_model = if config.agents.defaults.model.is_empty() {
        None
    } else {
        Some(config.agents.defaults.model.as_str())
    };
    let provider_name = config.agents.defaults.provider.as_str();
    let provider_config = match provider_name {
        "openrouter" => Some(&config.providers.openrouter),
        "openai" => Some(&config.providers.openai),
        "anthropic" => Some(&config.providers.anthropic),
        _ => None,
    };
    let selection = ProviderSelection::detect(
        requested_model,
        provider_config.map(|provider| provider.api_key.as_str()),
        provider_config.map(|provider| provider.api_base.as_str()),
    );
    match selection.kind {
        _ if provider_name == "demo_tool_calling" => Box::new(DemoToolCallingProvider),
        ProviderKind::OpenRouter | ProviderKind::OpenAI => {
            let provider_config = provider_config.cloned().unwrap_or_default();
            let api_base = if provider_config.api_base.is_empty() {
                match selection.kind {
                    ProviderKind::OpenRouter => "https://openrouter.ai/api/v1".to_string(),
                    ProviderKind::OpenAI => "https://api.openai.com/v1".to_string(),
                    _ => String::new(),
                }
            } else {
                provider_config.api_base
            };
            Box::new(OpenAiCompatibleProvider::new(
                provider_config.api_key,
                api_base,
                selection.model,
                CurlExecutor,
            ))
        }
        ProviderKind::Anthropic => Box::new(StaticProvider::new(selection.model, "anthropic")),
        ProviderKind::Offline => Box::new(StaticProvider::new(selection.model, "echo")),
    }
}

#[cfg(test)]
mod tests {
    use nanobot_config::Config;

    use super::{
        ChatMessage, ChatRequest, DemoToolCallingProvider, HttpExecutor, HttpRequest, LlmProvider,
        OpenAiCompatibleProvider, ProviderError, ProviderKind, ProviderSelection, StaticProvider,
        build_provider_from_config,
    };

    #[derive(Debug, Clone, Default)]
    struct RecordingExecutor {
        response: String,
        last_request: std::sync::Arc<std::sync::Mutex<Option<HttpRequest>>>,
    }

    impl RecordingExecutor {
        fn with_response(response: &str) -> Self {
            Self {
                response: response.to_string(),
                last_request: std::sync::Arc::new(std::sync::Mutex::new(None)),
            }
        }
    }

    impl HttpExecutor for RecordingExecutor {
        fn execute(&self, request: &HttpRequest) -> Result<String, ProviderError> {
            *self.last_request.lock().expect("lock") = Some(request.clone());
            Ok(self.response.clone())
        }
    }

    #[test]
    fn provider_registry_detects_openrouter_from_api_key_prefix() {
        let provider = ProviderSelection::detect(Some("gpt-4o-mini"), Some("sk-or-test"), None);
        assert_eq!(
            provider.model, "openrouter/auto",
            "stub should fail until provider registry exists"
        );
        assert_eq!(provider.kind, ProviderKind::OpenRouter);
    }

    #[test]
    fn static_provider_still_exposes_default_model() {
        let provider = StaticProvider::default();
        assert_eq!(provider.default_model(), "offline/echo");
    }

    #[test]
    fn config_builds_provider_from_agent_defaults() {
        let config = Config::from_json_str(
            r#"{
                "providers": {
                    "openrouter": { "apiKey": "sk-or-v1-123" }
                },
                "agents": {
                    "defaults": {
                        "model": "gpt-4o-mini",
                        "provider": "openrouter"
                    }
                }
            }"#,
        )
        .expect("config should parse");

        let provider = build_provider_from_config(&config);
        assert_eq!(provider.default_model(), "openrouter/auto");
    }

    #[test]
    fn openai_compatible_provider_builds_real_http_request() {
        let executor = RecordingExecutor::with_response(
            r#"{"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]}"#,
        );
        let provider = OpenAiCompatibleProvider::new(
            "sk-test",
            "https://api.openai.com/v1",
            "gpt-4o-mini",
            executor.clone(),
        );

        let response = provider
            .chat(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "Hi".to_string(),
                }],
                tools: vec!["filesystem".to_string()],
                model: None,
            })
            .expect("provider call should succeed");

        let request = executor
            .last_request
            .lock()
            .expect("lock")
            .clone()
            .expect("request should be recorded");
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
        assert!(request.body.contains("\"model\":\"gpt-4o-mini\""));
        assert!(request.body.contains("\"filesystem\""));
        assert_eq!(response.content.as_deref(), Some("hello"));
    }

    #[test]
    fn openai_compatible_provider_parses_tool_calls() {
        let executor = RecordingExecutor::with_response(
            r#"{
                "choices": [{
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "function": {
                                "name": "message",
                                "arguments": "{\"content\":\"hi\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        );
        let provider = OpenAiCompatibleProvider::new(
            "sk-test",
            "https://openrouter.ai/api/v1",
            "openrouter/auto",
            executor,
        );

        let response = provider
            .chat(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "Hi".to_string(),
                }],
                tools: vec!["message".to_string()],
                model: None,
            })
            .expect("provider call should succeed");

        assert_eq!(response.finish_reason, "tool_calls");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "message");
        assert_eq!(response.tool_calls[0].arguments["content"], "hi");
    }

    #[test]
    fn demo_tool_calling_provider_emits_tool_calls() {
        let provider = DemoToolCallingProvider;
        let response = provider
            .chat(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "please write a file".to_string(),
                }],
                tools: vec!["filesystem".to_string()],
                model: None,
            })
            .expect("provider should succeed");

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "filesystem");
    }
}

use serde_json::Value;

use super::LlmProvider;
use crate::{
    ChatRequest, HttpExecutor, HttpRequest, LlmResponse, ProviderError, ReqwestExecutor,
    ToolCallRequest,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider<E = ReqwestExecutor> {
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
            "messages": request.messages.iter().map(serialize_chat_message).collect::<Vec<_>>(),
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
        log::info!(
            "sending OpenAI-compatible request: method={} url={} headers={:?} body={}",
            http_request.method,
            http_request.url,
            http_request.headers,
            http_request.body
        );
        let raw = self.executor.execute(&http_request)?;
        log::info!("received OpenAI-compatible response: {raw}");
        self.parse_response(&raw)
    }

    fn default_model(&self) -> &str {
        &self.model
    }
}

fn serialize_chat_message(message: &crate::ChatMessage) -> Value {
    let mut value = serde_json::json!({
        "role": message.role,
    });

    if let Some(content) = &message.content {
        value["content"] = Value::String(content.clone());
    }

    if let Some(tool_call_id) = &message.tool_call_id {
        value["tool_call_id"] = Value::String(tool_call_id.clone());
    }

    if !message.tool_calls.is_empty() {
        value["tool_calls"] = Value::Array(
            message
                .tool_calls
                .iter()
                .map(|call| {
                    serde_json::json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments.to_string(),
                        }
                    })
                })
                .collect(),
        );
    }

    value
}

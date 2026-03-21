use super::LlmProvider;
use crate::{ChatRequest, LlmResponse, ProviderError, ToolCallRequest};

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

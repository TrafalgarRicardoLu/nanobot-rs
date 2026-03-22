use super::LlmProvider;
use crate::{ChatRequest, LlmResponse, ProviderError};

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
            content: Some(format!(
                "{}: {}",
                self.response_prefix,
                last_user.content.clone().unwrap_or_default()
            )),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        })
    }

    fn default_model(&self) -> &str {
        &self.model
    }
}

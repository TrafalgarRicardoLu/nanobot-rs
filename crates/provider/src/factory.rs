use nanobot_config::Config;

use crate::{OpenAiCompatibleProvider, ReqwestExecutor, providers::LlmProvider};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAI,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSelection {
    pub kind: ProviderKind,
    pub model: String,
}

impl ProviderSelection {
    pub fn detect(
        requested_model: Option<&str>,
        _api_key: Option<&str>,
        _api_base: Option<&str>,
    ) -> Self {
        Self {
            kind: ProviderKind::OpenAI,
            model: requested_model.unwrap_or("offline/echo").to_string(),
        }
    }
}

pub fn build_provider_from_config(config: &Config) -> Box<dyn LlmProvider> {
    let requested_model = if config.agents.defaults.model.is_empty() {
        None
    } else {
        Some(config.agents.defaults.model.as_str())
    };
    let provider_config = &config.providers.openai;
    let selection = ProviderSelection::detect(
        requested_model,
        Some(provider_config.api_key.as_str()),
        Some(provider_config.api_base.as_str()),
    );
    Box::new(OpenAiCompatibleProvider::new(
        provider_config.api_key.clone(),
        provider_config.api_base.clone(),
        selection.model,
        ReqwestExecutor,
    ))
}

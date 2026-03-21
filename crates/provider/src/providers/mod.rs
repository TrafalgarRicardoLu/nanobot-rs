mod demo_tool_calling;
mod openai_compatible;
mod static_provider;

pub use demo_tool_calling::DemoToolCallingProvider;
pub use openai_compatible::OpenAiCompatibleProvider;
pub use static_provider::StaticProvider;

use crate::{ChatRequest, LlmResponse, ProviderError};

pub trait LlmProvider: Send + Sync {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError>;
    fn default_model(&self) -> &str;
}

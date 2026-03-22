mod error;
mod factory;
mod http;
mod providers;
mod types;

pub use error::ProviderError;
pub use factory::{ProviderKind, ProviderSelection, build_provider_from_config};
pub use http::{CurlExecutor, HttpExecutor, HttpRequest, ReqwestExecutor};
pub use providers::{
    DemoToolCallingProvider, LlmProvider, OpenAiCompatibleProvider, StaticProvider,
};
pub use types::{ChatMessage, ChatRequest, LlmResponse, ToolCallMessage, ToolCallRequest};

#[cfg(test)]
mod tests;

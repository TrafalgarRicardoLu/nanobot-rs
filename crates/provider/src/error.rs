use thiserror::Error;

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

use nanobot_provider::ProviderError;
use nanobot_session::SessionError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("agent run cancelled")]
    Cancelled,
    #[error("agent run exceeded max steps: {0}")]
    MaxStepsExceeded(usize),
}

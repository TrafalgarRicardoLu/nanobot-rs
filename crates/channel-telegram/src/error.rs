use thiserror::Error;

#[derive(Debug, Error)]
pub enum TelegramChannelError {
    #[error("invalid telegram config: {0}")]
    InvalidConfig(String),
    #[error("telegram transport error: {0}")]
    Transport(String),
}

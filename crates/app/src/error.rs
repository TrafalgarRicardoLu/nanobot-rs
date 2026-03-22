use nanobot_core::AgentError;
use nanobot_cron::CronError;
use nanobot_session::SessionError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("agent error: {0}")]
    Agent(#[from] AgentError),
    #[error("channel error: {0}")]
    Channel(String),
    #[error("cron error: {0}")]
    Cron(#[from] CronError),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
}

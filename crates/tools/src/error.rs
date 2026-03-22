use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("invalid arguments for {tool}: {message}")]
    InvalidArguments { tool: String, message: String },
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("path escapes workspace root: {0}")]
    PathEscapesWorkspace(String),
}

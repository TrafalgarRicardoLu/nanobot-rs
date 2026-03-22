mod app;
mod background;
mod channels;
mod dispatch;
mod error;

pub use app::NanobotApp;
pub use background::BackgroundWorkerHandle;
pub use dispatch::DispatchRecord;
pub use error::AppError;

pub(crate) use channels::{build_builtin_channels, split_session_key};

#[cfg(test)]
mod tests;

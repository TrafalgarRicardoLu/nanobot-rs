use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use thiserror::Error;

pub use nanobot_bus::{InboundMessage, MessageBus, OutboundMessage};

pub type InboundPublisher = tokio::sync::mpsc::UnboundedSender<InboundMessage>;

pub trait Channel: Send + Sync {
    fn name(&self) -> &'static str;
    fn allow_from(&self) -> &[String];

    fn is_allowed(&self, sender_id: &str) -> bool {
        is_allowed(self.allow_from(), sender_id)
    }

    fn spawn_inbound_runtime(&self, _inbound_tx: InboundPublisher) -> Option<ChannelRuntimeHandle> {
        None
    }

    fn send(&self, _msg: &OutboundMessage) -> Result<(), ChannelError> {
        Err(ChannelError::UnsupportedOperation)
    }
}

pub fn is_allowed(allow_from: &[String], sender_id: &str) -> bool {
    if allow_from.is_empty() {
        return false;
    }
    if allow_from.iter().any(|entry| entry == "*") {
        return true;
    }
    if allow_from.iter().any(|entry| entry == sender_id) {
        return true;
    }
    sender_id
        .split('|')
        .any(|segment| allow_from.iter().any(|entry| entry == segment))
}

#[non_exhaustive]
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChannelError {
    #[error("unsupported operation")]
    UnsupportedOperation,
}

pub struct ChannelRuntimeHandle {
    name: &'static str,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stop_hook: Option<Box<dyn Fn() + Send + Sync>>,
}

impl ChannelRuntimeHandle {
    pub fn new(name: &'static str, stop: Arc<AtomicBool>, join: JoinHandle<()>) -> Self {
        Self::with_stop_hook(name, stop, join, Box::new(|| {}))
    }

    pub fn with_stop_hook(
        name: &'static str,
        stop: Arc<AtomicBool>,
        join: JoinHandle<()>,
        stop_hook: Box<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            name,
            stop,
            join: Some(join),
            stop_hook: Some(stop_hook),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(stop_hook) = &self.stop_hook {
            stop_hook();
        }
    }

    pub fn join(mut self) -> std::thread::Result<()> {
        self.join.take().expect("join handle").join()
    }
}

#[cfg(test)]
mod tests;

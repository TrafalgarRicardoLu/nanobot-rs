use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

pub trait Channel: Send + Sync {
    fn name(&self) -> &'static str;
    fn allow_from(&self) -> &[String];

    fn is_allowed(&self, sender_id: &str) -> bool {
        is_allowed(self.allow_from(), sender_id)
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

pub struct ChannelRuntimeHandle {
    name: &'static str,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl ChannelRuntimeHandle {
    pub fn new(name: &'static str, stop: Arc<AtomicBool>, join: JoinHandle<()>) -> Self {
        Self {
            name,
            stop,
            join: Some(join),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn join(mut self) -> std::thread::Result<()> {
        self.join.take().expect("join handle").join()
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed;

    #[test]
    fn empty_allow_list_denies_access() {
        assert!(!is_allowed(&[], "user-1"));
    }

    #[test]
    fn wildcard_and_segment_matching_are_allowed() {
        assert!(is_allowed(&["*".to_string()], "user-1"));
        assert!(is_allowed(&["thread-9".to_string()], "user-1|thread-9"));
    }
}

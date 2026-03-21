use super::{is_allowed, Channel, ChannelError, MessageBus, OutboundMessage};

struct StubChannel {
    name: &'static str,
    allow_from: Vec<String>,
}

impl Channel for StubChannel {
    fn name(&self) -> &'static str {
        self.name
    }

    fn allow_from(&self) -> &[String] {
        &self.allow_from
    }
}

#[test]
fn empty_allow_list_denies_access() {
    assert!(!is_allowed(&[], "user-1"));
}

#[test]
fn stub_channel_reports_name_and_allow_list() {
    let channel = StubChannel {
        name: "stub",
        allow_from: vec!["user-1".to_string()],
    };

    assert_eq!(channel.name(), "stub");
    assert_eq!(channel.allow_from(), &["user-1".to_string()]);
}

#[test]
fn is_allowed_supports_exact_wildcard_and_pipe_delimited_sender_matches() {
    let exact = vec!["user-1".to_string()];
    let wildcard = vec!["*".to_string()];
    let pipe_delimited = vec!["thread-9".to_string()];

    assert!(is_allowed(&exact, "user-1"));
    assert!(is_allowed(&wildcard, "user-1"));
    assert!(is_allowed(&pipe_delimited, "user-1|thread-9"));
}

#[test]
fn spawn_inbound_runtime_defaults_to_none() {
    let channel = StubChannel {
        name: "stub",
        allow_from: vec!["user-1".to_string()],
    };
    let bus = MessageBus::new();

    assert!(channel
        .spawn_inbound_runtime(bus.inbound_publisher())
        .is_none());
}

#[test]
fn send_defaults_to_unsupported_operation_error() {
    let channel = StubChannel {
        name: "stub",
        allow_from: vec!["user-1".to_string()],
    };
    let msg = OutboundMessage {
        channel: "stub".to_string(),
        chat_id: "chat-1".to_string(),
        content: "hello".to_string(),
        reply_to: None,
        metadata: Default::default(),
    };

    assert!(matches!(
        channel.send(&msg),
        Err(ChannelError::UnsupportedOperation)
    ));
}

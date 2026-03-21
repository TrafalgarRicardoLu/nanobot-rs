use std::collections::HashMap;

use super::{InboundMessage, MessageBus, OutboundMessage};

#[test]
fn inbound_message_prefers_session_key_override() {
    let message = InboundMessage {
        channel: "telegram".to_string(),
        sender_id: "user-1".to_string(),
        chat_id: "123".to_string(),
        content: "hello".to_string(),
        media: Vec::new(),
        metadata: HashMap::new(),
        session_key_override: Some("override:thread".to_string()),
    };

    assert_eq!(message.session_key(), "override:thread");
}

#[tokio::test]
async fn bus_round_trips_inbound_and_outbound_messages() {
    let mut bus = MessageBus::new();
    let inbound = InboundMessage {
        channel: "feishu".to_string(),
        sender_id: "ou_1".to_string(),
        chat_id: "oc_1".to_string(),
        content: "ping".to_string(),
        media: Vec::new(),
        metadata: HashMap::new(),
        session_key_override: None,
    };
    let outbound = OutboundMessage {
        channel: "qq".to_string(),
        chat_id: "user-9".to_string(),
        content: "pong".to_string(),
        reply_to: None,
        metadata: HashMap::new(),
    };

    bus.publish_inbound(inbound.clone())
        .expect("inbound should publish");
    bus.publish_outbound(outbound.clone())
        .expect("outbound should publish");

    assert_eq!(bus.consume_inbound().await, Some(inbound));
    assert_eq!(bus.consume_outbound().await, Some(outbound));
}

#[test]
fn bus_can_try_consume_outbound_without_async_runtime() {
    let mut bus: MessageBus = MessageBus::new();
    bus.publish_outbound(OutboundMessage {
        channel: "feishu".to_string(),
        chat_id: "oc_1".to_string(),
        content: "hello".to_string(),
        reply_to: None,
        metadata: HashMap::new(),
    })
    .expect("outbound should publish");

    assert!(
        bus.try_consume_outbound().is_some(),
        "stub should fail until try_consume_outbound exists"
    );
}

#[test]
fn bus_can_try_consume_inbound_without_async_runtime() {
    let mut bus = MessageBus::new();
    bus.publish_inbound(InboundMessage {
        channel: "qq".to_string(),
        sender_id: "user-1".to_string(),
        chat_id: "user-1".to_string(),
        content: "hello".to_string(),
        media: Vec::new(),
        metadata: HashMap::new(),
        session_key_override: None,
    })
    .expect("inbound should publish");

    assert!(
        bus.try_consume_inbound().is_some(),
        "stub should fail until try_consume_inbound exists"
    );
}

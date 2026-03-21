use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundMessage {
    pub channel: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
    pub media: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub session_key_override: Option<String>,
}

impl InboundMessage {
    pub fn session_key(&self) -> String {
        self.session_key_override
            .clone()
            .unwrap_or_else(|| format!("{}:{}", self.channel, self.chat_id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutboundMessage {
    pub channel: String,
    pub chat_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub metadata: HashMap<String, String>,
}

pub struct MessageBus {
    inbound_tx: UnboundedSender<InboundMessage>,
    inbound_rx: UnboundedReceiver<InboundMessage>,
    outbound_tx: UnboundedSender<OutboundMessage>,
    outbound_rx: UnboundedReceiver<OutboundMessage>,
}

impl MessageBus {
    pub fn new() -> Self {
        let (inbound_tx, inbound_rx) = unbounded_channel();
        let (outbound_tx, outbound_rx) = unbounded_channel();
        Self {
            inbound_tx,
            inbound_rx,
            outbound_tx,
            outbound_rx,
        }
    }

    pub fn publish_inbound(&self, msg: InboundMessage) -> Result<(), &'static str> {
        self.inbound_tx
            .send(msg)
            .map_err(|_| "inbound channel closed")
    }

    pub async fn consume_inbound(&mut self) -> Option<InboundMessage> {
        self.inbound_rx.recv().await
    }

    pub fn try_consume_inbound(&mut self) -> Option<InboundMessage> {
        match self.inbound_rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn inbound_publisher(&self) -> UnboundedSender<InboundMessage> {
        self.inbound_tx.clone()
    }

    pub fn publish_outbound(&self, msg: OutboundMessage) -> Result<(), &'static str> {
        self.outbound_tx
            .send(msg)
            .map_err(|_| "outbound channel closed")
    }

    pub async fn consume_outbound(&mut self) -> Option<OutboundMessage> {
        self.outbound_rx.recv().await
    }

    pub fn try_consume_outbound(&mut self) -> Option<OutboundMessage> {
        match self.outbound_rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
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
        let mut bus = MessageBus::new();
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
}

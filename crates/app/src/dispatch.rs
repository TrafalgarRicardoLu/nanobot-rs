use nanobot_bus::OutboundMessage;

use crate::NanobotApp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRecord {
    pub channel: String,
    pub chat_id: String,
    pub rendered: String,
    pub delivery: String,
}

impl NanobotApp {
    pub(crate) fn render_outbound(&self, msg: &OutboundMessage) -> String {
        msg.content.clone()
    }

    pub(crate) fn deliver_outbound(&self, msg: &OutboundMessage) -> String {
        match self
            .channels
            .iter()
            .find(|channel| channel.name() == msg.channel)
        {
            Some(channel) => match channel.send(msg) {
                Ok(_) => "sent".to_string(),
                Err(error) => format!("send_failed:{error}"),
            },
            None => "skipped:unsupported_channel".to_string(),
        }
    }
}

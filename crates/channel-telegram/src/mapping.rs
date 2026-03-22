use std::collections::HashMap;

use nanobot_bus::InboundMessage;

use crate::types::TelegramUpdate;

pub(crate) fn inbound_from_update(
    allow_from: &[String],
    channel_name: &str,
    update: TelegramUpdate,
) -> Option<InboundMessage> {
    let message = update.message?;
    let from = message.from?;
    if from.is_bot || message.chat.chat_type != "private" {
        return None;
    }
    let content = message.text?.trim().to_string();
    if content.is_empty() {
        return None;
    }
    let sender_id = from.id.to_string();
    if !nanobot_channels::is_allowed(allow_from, &sender_id) {
        return None;
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "telegram_update_id".to_string(),
        update.update_id.to_string(),
    );
    metadata.insert(
        "telegram_message_id".to_string(),
        message.message_id.to_string(),
    );
    if let Some(username) = from.username {
        metadata.insert("telegram_username".to_string(), username);
    }

    Some(InboundMessage {
        channel: channel_name.to_string(),
        sender_id,
        chat_id: message.chat.id.to_string(),
        content,
        media: Vec::new(),
        metadata,
        session_key_override: None,
    })
}

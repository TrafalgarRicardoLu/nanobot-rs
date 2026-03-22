use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GetUpdatesRequest {
    pub(crate) offset: Option<i64>,
    pub(crate) timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SendMessageRequest {
    pub(crate) chat_id: String,
    pub(crate) text: String,
    pub(crate) reply_to_message_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeleteWebhookRequest {
    pub(crate) drop_pending_updates: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TelegramApiEnvelope<T> {
    pub(crate) ok: bool,
    pub(crate) result: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TelegramUpdate {
    pub(crate) update_id: i64,
    pub(crate) message: Option<TelegramMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TelegramMessage {
    pub(crate) message_id: i64,
    pub(crate) text: Option<String>,
    pub(crate) chat: TelegramChat,
    pub(crate) from: Option<TelegramUser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TelegramChat {
    pub(crate) id: i64,
    #[serde(rename = "type")]
    pub(crate) chat_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TelegramUser {
    pub(crate) id: i64,
    pub(crate) is_bot: bool,
    pub(crate) username: Option<String>,
}

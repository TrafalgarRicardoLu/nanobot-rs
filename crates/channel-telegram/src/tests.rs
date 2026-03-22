use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use nanobot_bus::InboundMessage;
use nanobot_channels::{Channel, OutboundMessage};
use nanobot_config::ChannelConfig;
use serde_json::json;

use crate::api::{ReqwestTelegramApi, TelegramApi};
use crate::mapping::inbound_from_update;
use crate::settings::{TelegramSettings, default_api_base};
use crate::types::{
    DeleteWebhookRequest, GetUpdatesRequest, SendMessageRequest, TelegramChat, TelegramMessage,
    TelegramUpdate, TelegramUser,
};
use crate::{TELEGRAM_CHANNEL_NAME, TelegramChannel, TelegramChannelError};

#[derive(Debug, Default)]
struct FakeTelegramApi {
    updates: Mutex<Vec<Result<Vec<TelegramUpdate>, TelegramChannelError>>>,
    sent_messages: Mutex<Vec<SendMessageRequest>>,
    deleted_webhooks: Mutex<Vec<DeleteWebhookRequest>>,
}

impl FakeTelegramApi {
    fn queued(updates: Vec<Result<Vec<TelegramUpdate>, TelegramChannelError>>) -> Arc<Self> {
        Arc::new(Self {
            updates: Mutex::new(updates.into_iter().rev().collect()),
            ..Self::default()
        })
    }

    fn sent(&self) -> MutexGuard<'_, Vec<SendMessageRequest>> {
        self.sent_messages.lock().expect("sent messages lock")
    }

    fn deleted(&self) -> MutexGuard<'_, Vec<DeleteWebhookRequest>> {
        self.deleted_webhooks.lock().expect("deleted webhooks lock")
    }
}

impl TelegramApi for FakeTelegramApi {
    fn delete_webhook(&self, request: &DeleteWebhookRequest) -> Result<(), TelegramChannelError> {
        self.deleted_webhooks
            .lock()
            .expect("deleted webhooks lock")
            .push(request.clone());
        Ok(())
    }

    fn get_updates(
        &self,
        _request: &GetUpdatesRequest,
    ) -> Result<Vec<TelegramUpdate>, TelegramChannelError> {
        self.updates
            .lock()
            .expect("updates lock")
            .pop()
            .unwrap_or(Ok(Vec::new()))
    }

    fn send_message(&self, request: &SendMessageRequest) -> Result<(), TelegramChannelError> {
        self.sent_messages
            .lock()
            .expect("sent messages lock")
            .push(request.clone());
        Ok(())
    }
}

fn settings() -> TelegramSettings {
    TelegramSettings {
        bot_token: "123:test".to_string(),
        api_base: default_api_base(),
        poll_timeout_seconds: 0,
        drop_pending_updates_on_start: false,
    }
}

fn private_text_update(user_id: i64, chat_id: i64, text: &str) -> TelegramUpdate {
    TelegramUpdate {
        update_id: 101,
        message: Some(TelegramMessage {
            message_id: 202,
            text: Some(text.to_string()),
            chat: TelegramChat {
                id: chat_id,
                chat_type: "private".to_string(),
            },
            from: Some(TelegramUser {
                id: user_id,
                is_bot: false,
                username: Some("alice".to_string()),
            }),
        }),
    }
}

#[test]
fn from_config_requires_non_empty_bot_token() {
    let config = ChannelConfig {
        kind: TELEGRAM_CHANNEL_NAME.to_string(),
        enabled: true,
        allow_from: vec!["123".to_string()],
        settings: json!({
            "botToken": ""
        }),
    };

    let error = TelegramChannel::from_config(&config)
        .err()
        .expect("empty bot token must fail");

    assert!(error.to_string().contains("botToken"));
}

#[test]
fn inbound_mapping_keeps_private_text_messages() {
    let inbound = inbound_from_update(
        &["42".to_string()],
        TELEGRAM_CHANNEL_NAME,
        private_text_update(42, 9001, "hello telegram"),
    )
    .expect("private text update should map");

    assert_eq!(inbound.channel, TELEGRAM_CHANNEL_NAME);
    assert_eq!(inbound.sender_id, "42");
    assert_eq!(inbound.chat_id, "9001");
    assert_eq!(inbound.content, "hello telegram");
    assert_eq!(
        inbound
            .metadata
            .get("telegram_message_id")
            .map(String::as_str),
        Some("202")
    );
    assert_eq!(
        inbound
            .metadata
            .get("telegram_username")
            .map(String::as_str),
        Some("alice")
    );
}

#[test]
fn inbound_mapping_rejects_non_private_or_unauthorized_messages() {
    let mut update = private_text_update(7, 9001, "hello");
    update.message.as_mut().expect("message").chat.chat_type = "group".to_string();
    assert!(inbound_from_update(&["7".to_string()], TELEGRAM_CHANNEL_NAME, update).is_none());

    assert!(
        inbound_from_update(
            &["9".to_string()],
            TELEGRAM_CHANNEL_NAME,
            private_text_update(7, 9001, "hello"),
        )
        .is_none()
    );
}

#[test]
fn send_maps_reply_to_message_id() {
    let api = FakeTelegramApi::queued(Vec::new());
    let channel = TelegramChannel::with_api(vec!["42".to_string()], settings(), api.clone());

    channel
        .send(&OutboundMessage {
            channel: TELEGRAM_CHANNEL_NAME.to_string(),
            chat_id: "9001".to_string(),
            content: "reply".to_string(),
            reply_to: Some("55".to_string()),
            metadata: HashMap::new(),
        })
        .expect("send should succeed");

    let sent = api.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].chat_id, "9001");
    assert_eq!(sent[0].text, "reply");
    assert_eq!(sent[0].reply_to_message_id, Some(55));
}

#[test]
fn runtime_publishes_allowed_private_text_updates() {
    let api = FakeTelegramApi::queued(vec![Ok(vec![private_text_update(42, 9001, "ping")])]);
    let mut settings = settings();
    settings.drop_pending_updates_on_start = true;
    let channel = TelegramChannel::with_api(vec!["42".to_string()], settings, api.clone());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InboundMessage>();

    let handle = channel
        .spawn_inbound_runtime(tx)
        .expect("telegram runtime should start");
    let inbound = rx.blocking_recv().expect("inbound message should arrive");
    handle.stop();
    let _ = handle.join();

    assert_eq!(inbound.content, "ping");
    assert_eq!(api.deleted().len(), 1);
}

#[test]
#[ignore = "real network test; requires TELEGRAM_BOT_TOKEN and TELEGRAM_USER_ID"]
fn can_send_real_telegram_message_from_runtime_config() {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN must be set");
    let user_id = std::env::var("TELEGRAM_USER_ID").expect("TELEGRAM_USER_ID must be set");
    let config = ChannelConfig {
        kind: TELEGRAM_CHANNEL_NAME.to_string(),
        enabled: true,
        allow_from: vec![user_id.clone()],
        settings: json!({
            "botToken": bot_token,
            "apiBase": "https://api.telegram.org",
            "pollTimeoutSeconds": 5,
            "dropPendingUpdatesOnStart": true
        }),
    };
    let channel = TelegramChannel::from_config(&config).expect("channel should build");

    channel
        .send(&OutboundMessage {
            channel: TELEGRAM_CHANNEL_NAME.to_string(),
            chat_id: user_id,
            content: "hello from nanobot-rs test".to_string(),
            reply_to: None,
            metadata: HashMap::new(),
        })
        .expect("telegram send should succeed");
}

#[test]
#[ignore = "real network test; requires TELEGRAM_BOT_TOKEN and TELEGRAM_USER_ID"]
fn can_receive_real_telegram_message_and_print_mapping() {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN must be set");
    let user_id = std::env::var("TELEGRAM_USER_ID").expect("TELEGRAM_USER_ID must be set");
    let timeout_seconds = std::env::var("TELEGRAM_POLL_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20);
    let settings = TelegramSettings {
        bot_token,
        api_base: default_api_base(),
        poll_timeout_seconds: timeout_seconds,
        drop_pending_updates_on_start: true,
    };
    let api = ReqwestTelegramApi::new(settings.clone()).expect("api should build");

    api.delete_webhook(&DeleteWebhookRequest {
        drop_pending_updates: true,
    })
    .expect("deleteWebhook should succeed");

    let updates = api
        .get_updates(&GetUpdatesRequest {
            offset: None,
            timeout_seconds: settings.poll_timeout_seconds,
        })
        .expect("getUpdates should succeed");

    let inbound = updates
        .into_iter()
        .filter_map(|update| {
            inbound_from_update(
                std::slice::from_ref(&user_id),
                TELEGRAM_CHANNEL_NAME,
                update,
            )
        })
        .next()
        .expect("expected one authorized private text update");

    println!(
        "received inbound: channel={} sender_id={} chat_id={} content={:?} metadata={:?}",
        inbound.channel, inbound.sender_id, inbound.chat_id, inbound.content, inbound.metadata
    );
}

use std::time::Duration;

use reqwest::blocking::Client;

use crate::error::TelegramChannelError;
use crate::settings::TelegramSettings;
use crate::types::{
    DeleteWebhookRequest, GetUpdatesRequest, SendMessageRequest, TelegramApiEnvelope,
    TelegramUpdate,
};

pub(crate) trait TelegramApi: Send + Sync {
    fn delete_webhook(&self, request: &DeleteWebhookRequest) -> Result<(), TelegramChannelError>;
    fn get_updates(
        &self,
        request: &GetUpdatesRequest,
    ) -> Result<Vec<TelegramUpdate>, TelegramChannelError>;
    fn send_message(&self, request: &SendMessageRequest) -> Result<(), TelegramChannelError>;
}

#[derive(Debug)]
pub(crate) struct ReqwestTelegramApi {
    client: Client,
    settings: TelegramSettings,
}

impl ReqwestTelegramApi {
    pub(crate) fn new(settings: TelegramSettings) -> Result<Self, TelegramChannelError> {
        let timeout_seconds = settings.poll_timeout_seconds.saturating_add(5).max(5);
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .map_err(|error| TelegramChannelError::Transport(error.to_string()))?;
        Ok(Self { client, settings })
    }

    fn method_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.settings.api_base.trim_end_matches('/'),
            self.settings.bot_token,
            method
        )
    }
}

impl TelegramApi for ReqwestTelegramApi {
    fn delete_webhook(&self, request: &DeleteWebhookRequest) -> Result<(), TelegramChannelError> {
        let body = serde_json::json!({
            "drop_pending_updates": request.drop_pending_updates,
        });
        self.client
            .post(self.method_url("deleteWebhook"))
            .json(&body)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| TelegramChannelError::Transport(error.to_string()))?;
        Ok(())
    }

    fn get_updates(
        &self,
        request: &GetUpdatesRequest,
    ) -> Result<Vec<TelegramUpdate>, TelegramChannelError> {
        let body = serde_json::json!({
            "offset": request.offset,
            "timeout": request.timeout_seconds,
            "allowed_updates": ["message"],
        });
        let response = self
            .client
            .post(self.method_url("getUpdates"))
            .json(&body)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| TelegramChannelError::Transport(error.to_string()))?;
        let envelope: TelegramApiEnvelope<Vec<TelegramUpdate>> = response
            .json()
            .map_err(|error| TelegramChannelError::Transport(error.to_string()))?;
        if envelope.ok {
            Ok(envelope.result)
        } else {
            Err(TelegramChannelError::Transport(
                "telegram getUpdates returned ok=false".to_string(),
            ))
        }
    }

    fn send_message(&self, request: &SendMessageRequest) -> Result<(), TelegramChannelError> {
        let body = serde_json::json!({
            "chat_id": request.chat_id,
            "text": request.text,
            "reply_to_message_id": request.reply_to_message_id,
        });
        self.client
            .post(self.method_url("sendMessage"))
            .json(&body)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| TelegramChannelError::Transport(error.to_string()))?;
        Ok(())
    }
}

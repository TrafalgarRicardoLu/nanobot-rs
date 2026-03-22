mod api;
mod error;
mod mapping;
mod settings;
mod types;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use api::{ReqwestTelegramApi, TelegramApi};
pub use error::TelegramChannelError;
use log::info;
use mapping::inbound_from_update;
use nanobot_channels::{
    Channel, ChannelError, ChannelRuntimeHandle, InboundPublisher, OutboundMessage,
};
use nanobot_config::ChannelConfig;
use settings::TelegramSettings;
use types::{DeleteWebhookRequest, GetUpdatesRequest, SendMessageRequest};

pub(crate) const TELEGRAM_CHANNEL_NAME: &str = "telegram";

pub struct TelegramChannel {
    allow_from: Vec<String>,
    settings: TelegramSettings,
    api: Arc<dyn TelegramApi>,
}

impl TelegramChannel {
    pub fn from_config(config: &ChannelConfig) -> Result<Self, TelegramChannelError> {
        let settings: TelegramSettings = serde_json::from_value(config.settings.clone())
            .map_err(|error| TelegramChannelError::InvalidConfig(error.to_string()))?;
        let settings = settings.validate()?;
        let api = Arc::new(ReqwestTelegramApi::new(settings.clone())?);
        Ok(Self {
            allow_from: config.allow_from.clone(),
            settings,
            api,
        })
    }

    #[cfg(test)]
    fn with_api(
        allow_from: Vec<String>,
        settings: TelegramSettings,
        api: Arc<dyn TelegramApi>,
    ) -> Self {
        Self {
            allow_from,
            settings,
            api,
        }
    }
}

impl Channel for TelegramChannel {
    fn name(&self) -> &'static str {
        TELEGRAM_CHANNEL_NAME
    }

    fn allow_from(&self) -> &[String] {
        &self.allow_from
    }

    fn spawn_inbound_runtime(&self, inbound_tx: InboundPublisher) -> Option<ChannelRuntimeHandle> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        let api = self.api.clone();
        let allow_from = self.allow_from.clone();
        let settings = self.settings.clone();
        let join = thread::spawn(move || {
            let mut offset = None;
            if settings.drop_pending_updates_on_start {
                let _ = api.delete_webhook(&DeleteWebhookRequest {
                    drop_pending_updates: true,
                });
            }
            while !stop_worker.load(Ordering::SeqCst) {
                match api.get_updates(&GetUpdatesRequest {
                    offset,
                    timeout_seconds: settings.poll_timeout_seconds,
                }) {
                    Ok(updates) => {
                        info!("telegram inbound poll returned updates={updates:?}");
                        for update in updates {
                            offset = Some(update.update_id + 1);
                            info!("telegram inbound raw update={update:?}");
                            if let Some(inbound) =
                                inbound_from_update(&allow_from, TELEGRAM_CHANNEL_NAME, update)
                            {
                                info!("telegram inbound mapped message={inbound:?}");
                                if inbound_tx.send(inbound).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        info!("telegram inbound poll error={error}");
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            }
        });
        Some(ChannelRuntimeHandle::new(self.name(), stop, join))
    }

    fn send(&self, msg: &OutboundMessage) -> Result<(), ChannelError> {
        info!("telegram outbound send request={msg:?}");
        if msg.content.trim().is_empty() {
            return Err(ChannelError::InvalidMessage(
                "telegram outbound content must not be empty".to_string(),
            ));
        }
        let reply_to_message_id = msg
            .reply_to
            .as_deref()
            .map(|raw| {
                raw.parse::<i64>().map_err(|_| {
                    ChannelError::InvalidMessage(format!(
                        "telegram reply_to must be a numeric message id, got {raw}"
                    ))
                })
            })
            .transpose()?;
        self.api
            .send_message(&SendMessageRequest {
                chat_id: msg.chat_id.clone(),
                text: msg.content.clone(),
                reply_to_message_id,
            })
            .map(|_| {
                info!("telegram outbound send completed message={msg:?}");
            })
            .map_err(|error| ChannelError::Transport(error.to_string()))
    }
}

#[cfg(test)]
mod tests;

use serde::Deserialize;

use crate::error::TelegramChannelError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelegramSettings {
    pub(crate) bot_token: String,
    #[serde(default = "default_api_base")]
    pub(crate) api_base: String,
    #[serde(default = "default_poll_timeout_seconds")]
    pub(crate) poll_timeout_seconds: u64,
    #[serde(default)]
    pub(crate) drop_pending_updates_on_start: bool,
}

impl TelegramSettings {
    pub(crate) fn validate(self) -> Result<Self, TelegramChannelError> {
        if self.bot_token.trim().is_empty() {
            return Err(TelegramChannelError::InvalidConfig(
                "settings.botToken must not be empty".to_string(),
            ));
        }
        if self.api_base.trim().is_empty() {
            return Err(TelegramChannelError::InvalidConfig(
                "settings.apiBase must not be empty".to_string(),
            ));
        }
        Ok(self)
    }
}

pub(crate) fn default_api_base() -> String {
    "https://api.telegram.org".to_string()
}

pub(crate) fn default_poll_timeout_seconds() -> u64 {
    20
}

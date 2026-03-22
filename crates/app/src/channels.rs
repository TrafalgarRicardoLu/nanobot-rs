use nanobot_channel_telegram::TelegramChannel;
use nanobot_channels::Channel;
use nanobot_config::Config;

use crate::AppError;

pub(crate) fn build_builtin_channels(config: &Config) -> Result<Vec<Box<dyn Channel>>, AppError> {
    let mut channels: Vec<Box<dyn Channel>> = Vec::new();
    for channel in &config.channels {
        if !channel.enabled {
            continue;
        }
        if channel.kind == "telegram" {
            let telegram = TelegramChannel::from_config(channel)
                .map_err(|error| AppError::Channel(error.to_string()))?;
            channels.push(Box::new(telegram));
        }
    }
    Ok(channels)
}

pub(crate) fn split_session_key(session_key: &str) -> (String, String) {
    match session_key.split_once(':') {
        Some((channel, chat_id)) => (channel.to_string(), chat_id.to_string()),
        None => ("cli".to_string(), session_key.to_string()),
    }
}

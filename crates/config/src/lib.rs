use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub providers: ProvidersConfig,
    pub agents: AgentsConfig,
    pub channels: ChannelsConfig,
}

impl Config {
    pub fn from_json_str(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }

    pub fn from_json_file(path: impl AsRef<std::path::Path>) -> Result<Self, std::io::Error> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_json_str(&raw)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ProvidersConfig {
    pub openrouter: ProviderConfig,
    pub openai: ProviderConfig,
    pub anthropic: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderConfig {
    pub api_key: String,
    pub api_base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentsConfig {
    pub defaults: AgentDefaults,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentDefaults {
    pub model: String,
    pub provider: String,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            model: "offline/echo".to_string(),
            provider: "offline".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ChannelsConfig {
    pub feishu: FeishuConfig,
    pub qq: QQConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct FeishuConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    pub encrypt_key: String,
    pub verification_token: String,
    pub websocket_url: String,
    pub allow_from: Vec<String>,
    pub react_emoji: String,
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            app_id: String::new(),
            app_secret: String::new(),
            encrypt_key: String::new(),
            verification_token: String::new(),
            websocket_url: String::new(),
            allow_from: Vec::new(),
            react_emoji: "THUMBSUP".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct QQConfig {
    pub enabled: bool,
    pub app_id: String,
    pub secret: String,
    pub websocket_url: String,
    pub allow_from: Vec<String>,
}

#[cfg(test)]
mod tests;

use serde::{
    de::{Error as DeError, Deserializer},
    Deserialize, Serialize,
};

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

pub type ChannelsConfig = Vec<ChannelConfig>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    #[serde(deserialize_with = "deserialize_channel_kind")]
    pub kind: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow_from: Vec<String>,
}

fn deserialize_channel_kind<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let kind = String::deserialize(deserializer)?;
    if kind.trim().is_empty() {
        return Err(D::Error::custom("channel kind must not be empty"));
    }

    Ok(kind)
}

#[cfg(test)]
mod tests;

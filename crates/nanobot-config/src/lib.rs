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
mod tests {
    use super::Config;

    #[test]
    fn parses_camel_case_channel_config() {
        let input = r#"{
            "providers": {
                "openrouter": {
                    "apiKey": "sk-or-v1-test"
                }
            },
            "agents": {
                "defaults": {
                    "model": "gpt-4o-mini",
                    "provider": "openrouter"
                }
            },
            "channels": {
                "feishu": {
                    "enabled": true,
                    "appId": "cli_a",
                    "appSecret": "secret_a",
                    "websocketUrl": "ws://127.0.0.1:3012/feishu",
                    "allowFrom": ["ou_1"],
                    "reactEmoji": "DONE"
                },
                "qq": {
                    "enabled": true,
                    "appId": "10001",
                    "secret": "qq-secret",
                    "websocketUrl": "ws://127.0.0.1:3012/qq",
                    "allowFrom": ["user-1"]
                }
            }
        }"#;

        let config = Config::from_json_str(input).expect("config should parse");
        let rendered = format!("{config:?}");

        assert!(
            rendered.contains("cli_a"),
            "expected Feishu appId to be parsed"
        );
        assert!(
            rendered.contains("sk-or-v1-test"),
            "expected provider apiKey to be parsed"
        );
        assert!(
            rendered.contains("gpt-4o-mini"),
            "expected agent default model to be parsed"
        );
        assert!(
            rendered.contains("DONE"),
            "expected Feishu reactEmoji to be parsed"
        );
        assert!(
            rendered.contains("ws://127.0.0.1:3012/feishu"),
            "expected Feishu websocketUrl to be parsed"
        );
        assert!(rendered.contains("10001"), "expected QQ appId to be parsed");
        assert!(
            rendered.contains("ws://127.0.0.1:3012/qq"),
            "expected QQ websocketUrl to be parsed"
        );
        assert!(
            rendered.contains("user-1"),
            "expected allowFrom entry to be parsed"
        );
    }

    #[test]
    fn applies_defaults_for_missing_sections() {
        let config = Config::from_json_str("{}").expect("config should parse");
        let rendered = format!("{config:?}");

        assert!(
            rendered.contains("enabled: false"),
            "channels should default to disabled"
        );
        assert!(
            rendered.contains("offline/echo"),
            "agents defaults should be present"
        );
        assert!(
            rendered.contains("react_emoji"),
            "default struct should keep Feishu defaults"
        );
        assert!(
            rendered.contains("websocket_url: \"\""),
            "websocket url should default to empty"
        );
    }
}

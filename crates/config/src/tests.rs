use super::Config;

#[test]
fn parses_generic_channel_list() {
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
        "channels": [
            {
                "kind": "stub",
                "enabled": true,
                "allowFrom": ["user-1"]
            }
        ]
    }"#;

    let config = Config::from_json_str(input).expect("config should parse");

    assert_eq!(config.providers.openrouter.api_key, "sk-or-v1-test");
    assert_eq!(config.agents.defaults.model, "gpt-4o-mini");
    assert_eq!(config.agents.defaults.provider, "openrouter");
    assert_eq!(config.channels.len(), 1);
    assert_eq!(config.channels[0].kind, "stub");
    assert!(config.channels[0].enabled);
    assert_eq!(config.channels[0].allow_from, vec!["user-1".to_string()]);
}

#[test]
fn rejects_channel_entries_with_empty_kind() {
    let input = r#"{
        "channels": [
            {
                "kind": "",
                "enabled": true,
                "allowFrom": ["user-1"]
            }
        ]
    }"#;

    let error = Config::from_json_str(input).expect_err("config should reject empty channel kind");

    assert!(
        error.to_string().contains("kind"),
        "expected deserialization error to mention kind"
    );
}

#[test]
fn rejects_channel_entries_missing_kind() {
    let input = r#"{
        "channels": [
            {
                "enabled": true,
                "allowFrom": ["user-1"]
            }
        ]
    }"#;

    let error = Config::from_json_str(input).expect_err("config should reject missing channel kind");

    assert!(
        error.to_string().contains("kind"),
        "expected deserialization error to mention kind"
    );
}

#[test]
fn defaults_produce_empty_channel_list() {
    let config = Config::from_json_str("{}").expect("config should parse");

    assert!(config.channels.is_empty(), "channels should default to an empty list");
}

#[test]
fn provider_and_agent_defaults_still_parse() {
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
        }
    }"#;

    let config = Config::from_json_str(input).expect("config should parse");

    assert_eq!(config.providers.openrouter.api_key, "sk-or-v1-test");
    assert_eq!(config.agents.defaults.model, "gpt-4o-mini");
    assert_eq!(config.agents.defaults.provider, "openrouter");
}

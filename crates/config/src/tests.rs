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

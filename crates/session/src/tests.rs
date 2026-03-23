use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::{Session, SessionManager, StoredMessage, StoredToolCall};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nanobot-rs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir should exist");
    dir
}

#[test]
fn saves_and_loads_jsonl_session_with_metadata_header() {
    let dir = temp_dir("save-load");
    let manager = SessionManager::new(&dir).expect("manager should build");
    let mut session = Session::new("feishu:oc_1");
    session
        .metadata
        .insert("channel".to_string(), "feishu".to_string());
    session.add_message("user", "hello");
    session.add_message("assistant", "world");

    let path = manager.save(&session).expect("session should save");
    let raw = fs::read_to_string(&path).expect("session file should exist");

    assert!(
        raw.lines()
            .next()
            .unwrap_or_default()
            .contains(r#""_type":"metadata""#)
    );

    let loaded = manager
        .load("feishu:oc_1")
        .expect("session should load")
        .expect("session should exist");
    assert_eq!(loaded.key, "feishu:oc_1");
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(
        loaded.metadata.get("channel").map(String::as_str),
        Some("feishu")
    );
}

#[test]
fn get_history_preserves_compact_summary_before_latest_turn() {
    let mut session = Session::new("qq:user-1");
    session.add_structured_message(StoredMessage {
        role: "system".to_string(),
        content: Some("summary of earlier context".to_string()),
        timestamp: "0".to_string(),
        name: None,
        tool_call_id: None,
        tool_calls: Vec::new(),
        metadata: HashMap::from([("kind".to_string(), "compact_summary".to_string())]),
    });
    session.add_message("assistant", "orphan assistant");
    session.add_message("tool", "orphan tool");
    session.add_message("user", "hello");
    session.add_message("assistant", "hi");

    let history = session.get_history(10);

    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, "system");
    assert_eq!(
        history[0].content.as_deref(),
        Some("summary of earlier context")
    );
    assert_eq!(history[1].role, "user");
    assert_eq!(history[1].content.as_deref(), Some("hello"));
    assert_eq!(history[2].role, "assistant");
}

#[test]
fn saves_and_loads_structured_message_fields() {
    let dir = temp_dir("structured");
    let manager = SessionManager::new(&dir).expect("manager should build");
    let mut session = Session::new("cli:structured");
    session.add_structured_message(StoredMessage {
        role: "assistant".to_string(),
        content: Some("tool call".to_string()),
        timestamp: "1".to_string(),
        name: Some("assistant".to_string()),
        tool_call_id: Some("call-1".to_string()),
        tool_calls: vec![StoredToolCall {
            id: "call-1".to_string(),
            name: "filesystem".to_string(),
            arguments: serde_json::json!({"path": "demo.txt"}),
        }],
        metadata: HashMap::from([("kind".to_string(), "tool".to_string())]),
    });

    manager.save(&session).expect("save should work");
    let loaded = manager
        .load("cli:structured")
        .expect("load should work")
        .expect("session should exist");

    assert_eq!(loaded.messages[0].tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(
        loaded.messages[0]
            .tool_calls
            .iter()
            .map(|call| call.name.clone())
            .collect::<Vec<_>>(),
        vec!["filesystem".to_string()]
    );
    assert_eq!(
        loaded.messages[0].metadata.get("kind").map(String::as_str),
        Some("tool")
    );
}

#[test]
fn save_and_load_round_trips_compact_summary_metadata() {
    let dir = temp_dir("compact-summary");
    let manager = SessionManager::new(&dir).expect("manager should build");
    let mut session = Session::new("qq:user-9");
    session.add_structured_message(StoredMessage {
        role: "system".to_string(),
        content: Some("summary: track todos and use filesystem".to_string()),
        timestamp: "1".to_string(),
        name: None,
        tool_call_id: None,
        tool_calls: vec![StoredToolCall {
            id: "call-2".to_string(),
            name: "filesystem".to_string(),
            arguments: serde_json::json!({}),
        }],
        metadata: HashMap::from([("kind".to_string(), "compact_summary".to_string())]),
    });
    session.add_message("user", "what's left?");

    manager.save(&session).expect("save should work");
    let loaded = manager
        .load("qq:user-9")
        .expect("load should work")
        .expect("session should exist");

    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(loaded.messages[0].role, "system");
    assert_eq!(
        loaded.messages[0].metadata.get("kind").map(String::as_str),
        Some("compact_summary")
    );
    assert_eq!(loaded.messages[1].content.as_deref(), Some("what's left?"));
}

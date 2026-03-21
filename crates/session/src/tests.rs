use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::{Session, SessionManager, StoredMessage};

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
fn get_history_skips_leading_non_user_messages_after_consolidation() {
    let mut session = Session::new("qq:user-1");
    session.add_message("assistant", "orphan assistant");
    session.add_message("tool", "orphan tool");
    session.add_message("user", "hello");
    session.add_message("assistant", "hi");
    session.last_consolidated = 1;

    let history = session.get_history(10);

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, "user");
    assert_eq!(history[0].content, "hello");
    assert_eq!(history[1].role, "assistant");
}

#[test]
fn saves_and_loads_structured_message_fields() {
    let dir = temp_dir("structured");
    let manager = SessionManager::new(&dir).expect("manager should build");
    let mut session = Session::new("cli:structured");
    session.add_structured_message(StoredMessage {
        role: "assistant".to_string(),
        content: "tool call".to_string(),
        timestamp: "1".to_string(),
        name: Some("assistant".to_string()),
        tool_call_id: Some("call-1".to_string()),
        tool_calls: vec!["filesystem".to_string()],
        metadata: HashMap::from([("kind".to_string(), "tool".to_string())]),
    });

    manager.save(&session).expect("save should work");
    let loaded = manager
        .load("cli:structured")
        .expect("load should work")
        .expect("session should exist");

    assert_eq!(loaded.messages[0].tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(
        loaded.messages[0].tool_calls,
        vec!["filesystem".to_string()]
    );
    assert_eq!(
        loaded.messages[0].metadata.get("kind").map(String::as_str),
        Some("tool")
    );
}

#[test]
fn consolidate_writes_memory_and_history_files() {
    let dir = temp_dir("consolidate");
    let manager = SessionManager::new(&dir).expect("manager should build");
    let mut session = Session::new("qq:user-9");
    session.add_message("user", "track my todos");
    session.add_structured_message(StoredMessage {
        role: "assistant".to_string(),
        content: "used filesystem".to_string(),
        timestamp: "2".to_string(),
        name: None,
        tool_call_id: None,
        tool_calls: vec!["filesystem".to_string()],
        metadata: HashMap::new(),
    });

    manager
        .consolidate(&mut session)
        .expect("consolidation should work");

    let memory = fs::read_to_string(manager.memory_dir("qq:user-9").join("MEMORY.md"))
        .expect("memory file should exist");
    let history = fs::read_to_string(manager.memory_dir("qq:user-9").join("HISTORY.md"))
        .expect("history file should exist");
    assert!(memory.contains("filesystem"));
    assert!(history.contains("track my todos"));
    assert_eq!(session.last_consolidated, session.messages.len());
}

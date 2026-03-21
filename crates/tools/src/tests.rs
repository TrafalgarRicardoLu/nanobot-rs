use super::ToolRegistry;

#[test]
fn builtin_registry_contains_expected_tool_names() {
    let names = ToolRegistry::with_builtin_defaults().names();
    assert!(names.contains(&"shell".to_string()));
    assert!(names.contains(&"filesystem".to_string()));
    assert!(names.contains(&"web".to_string()));
    assert!(names.contains(&"message".to_string()));
    assert!(names.contains(&"spawn".to_string()));
    assert!(names.contains(&"cron".to_string()));
    assert!(names.contains(&"mcp".to_string()));
}

#[test]
fn shell_tool_executes_echo_command() {
    let mut registry = ToolRegistry::with_builtin_defaults();
    let result = registry
        .execute("shell", serde_json::json!({"command": "echo tool-ok"}))
        .expect("shell tool should execute");
    assert!(
        result.contains("tool-ok"),
        "stub should fail until tool execution exists"
    );
}

#[test]
fn cron_tool_adds_job_and_reports_it() {
    let mut registry = ToolRegistry::with_builtin_defaults();
    let result = registry
        .execute(
            "cron",
            serde_json::json!({"action": "add", "name": "digest", "interval": 5}),
        )
        .expect("cron tool should execute");
    assert!(
        result.contains("digest"),
        "stub should fail until cron tool execution exists"
    );
}

#[test]
fn filesystem_tool_writes_and_reads_workspace_file() {
    let root = std::env::temp_dir().join(format!("nanobot-tools-fs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("workspace dir should exist");

    let mut registry = ToolRegistry::with_builtin_defaults();
    registry.set_workspace_root(root.clone());

    let write_result = registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "write", "path": "notes/todo.txt", "content": "ship it"}),
        )
        .expect("filesystem write should execute");
    assert!(
        write_result.contains("todo.txt"),
        "stub should fail until filesystem write exists"
    );

    let read_result = registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "read", "path": "notes/todo.txt"}),
        )
        .expect("filesystem read should execute");
    assert_eq!(read_result, "ship it");
}

#[test]
fn filesystem_tool_supports_append_exists_replace_and_delete() {
    let root = std::env::temp_dir().join(format!("nanobot-tools-rich-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("workspace dir should exist");

    let mut registry = ToolRegistry::with_builtin_defaults();
    registry.set_workspace_root(root);

    registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "write", "path": "notes/demo.txt", "content": "hello"}),
        )
        .expect("write should work");
    registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "append", "path": "notes/demo.txt", "content": " world"}),
        )
        .expect("append should work");
    let exists = registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "exists", "path": "notes/demo.txt"}),
        )
        .expect("exists should work");
    assert_eq!(exists, "true");

    registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "replace", "path": "notes/demo.txt", "old": "world", "new": "rust"}),
        )
        .expect("replace should work");
    let content = registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "read", "path": "notes/demo.txt"}),
        )
        .expect("read should work");
    assert_eq!(content, "hello rust");

    registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "delete", "path": "notes/demo.txt"}),
        )
        .expect("delete should work");
    let exists_after_delete = registry
        .execute(
            "filesystem",
            serde_json::json!({"action": "exists", "path": "notes/demo.txt"}),
        )
        .expect("exists should work");
    assert_eq!(exists_after_delete, "false");
}

#[test]
fn message_tool_queues_outbound_message_for_current_target() {
    let mut registry = ToolRegistry::with_builtin_defaults();
    registry.set_message_target("qq", "user-9");

    let result = registry
        .execute("message", serde_json::json!({"content": "queued reply"}))
        .expect("message tool should execute");

    assert!(
        result.contains("queued"),
        "stub should fail until message tool queues outbound messages"
    );
    assert_eq!(registry.take_outbound_messages().len(), 1);
}

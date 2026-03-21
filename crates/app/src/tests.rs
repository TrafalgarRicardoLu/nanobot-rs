use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use nanobot_config::{Config, FeishuConfig, QQConfig};
use nanobot_cron::CronError;
use nanobot_provider::{
    ChatRequest, DemoToolCallingProvider, LlmProvider, LlmResponse, ProviderError, StaticProvider,
    ToolCallRequest,
};
use serde_json::json;

use crate::NanobotApp;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nanobot-app-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir should exist");
    dir
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn write_bridge(dir: &Path, source: &str) -> PathBuf {
    let path = dir.join("qq_bridge_stub.py");
    fs::write(&path, source).expect("bridge source should write");
    path
}

fn with_bridge_source<T>(source: &str, test: impl FnOnce() -> T) -> T {
    let _guard = env_lock().lock().expect("env lock");
    let dir = temp_dir("bridge");
    let path = write_bridge(&dir, source);
    unsafe {
        std::env::set_var("NANOBOT_QQ_BRIDGE_SOURCE", &path);
    }
    let result = test();
    unsafe {
        std::env::remove_var("NANOBOT_QQ_BRIDGE_SOURCE");
    }
    result
}

#[test]
fn registers_only_enabled_feishu_and_qq_channels() {
    let mut config = Config::default();
    config.channels.feishu = FeishuConfig {
        enabled: true,
        allow_from: vec!["ou_1".to_string()],
        ..FeishuConfig::default()
    };
    config.channels.qq = QQConfig {
        enabled: true,
        allow_from: vec!["user-1".to_string()],
        ..QQConfig::default()
    };

    let app = NanobotApp::new(
        config,
        Box::new(StaticProvider::default()),
        temp_dir("channels"),
    )
    .expect("app should build");

    assert_eq!(app.enabled_channel_names(), vec!["feishu", "qq"]);
}

#[test]
fn handles_cli_messages_and_persists_session() {
    let dir = temp_dir("session");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");

    let response = app
        .handle_cli_message("cli:local", "hello")
        .expect("message should be handled");

    assert_eq!(response.as_deref(), Some("assistant: hello"));
    assert!(dir.join("sessions").join("cli_local.jsonl").exists());
}

#[test]
fn status_includes_model_and_enabled_channels() {
    let mut config = Config::default();
    config.channels.feishu = FeishuConfig {
        enabled: true,
        allow_from: vec!["ou_1".to_string()],
        ..FeishuConfig::default()
    };

    let app = NanobotApp::new(
        config,
        Box::new(StaticProvider::new("offline/test", "assistant")),
        temp_dir("status"),
    )
    .expect("app should build");

    let status = app.status_summary();
    assert!(
        status.contains("feishu"),
        "stub should fail until status summary exists"
    );
    assert!(
        status.contains("offline/test"),
        "stub should fail until provider model is exposed"
    );
}

#[test]
fn from_config_uses_agent_default_provider_model() {
    let config = Config::from_json_str(
        r#"{
            "providers": {
                "openai": {
                    "apiKey": "sk-test",
                    "apiBase": "https://example.com/v1"
                }
            },
            "agents": {
                "defaults": {
                    "model": "gpt-4o-mini",
                    "provider": "openai"
                }
            }
        }"#,
    )
    .expect("config should parse");

    let app = NanobotApp::from_config(config, temp_dir("provider")).expect("app should build");
    assert!(app.status_summary().contains("gpt-4o-mini"));
}

#[test]
fn cli_message_run_publishes_outbound_messages_from_tools() {
    let dir = temp_dir("outbound");
    let mut app = NanobotApp::new(Config::default(), Box::new(MessageToolProvider), &dir)
        .expect("app should build");

    let _ = app
        .handle_cli_message("qq:user-9", "queue a reply")
        .expect("message should be handled");

    let published = app.take_outbound_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].channel, "qq");
    assert_eq!(published[0].chat_id, "user-9");
    assert_eq!(published[0].content, "queued reply");
}

#[test]
fn demo_tool_calling_provider_can_write_files_via_cli_flow() {
    let dir = temp_dir("demo-tool-provider");
    let mut app = NanobotApp::new(Config::default(), Box::new(DemoToolCallingProvider), &dir)
        .expect("app should build");
    let response = app
        .handle_cli_message("cli:local", "write a file")
        .expect("message should be handled");

    assert!(response.unwrap_or_default().contains("demo complete"));
    let written = std::fs::read_to_string(dir.join("demo").join("generated.txt"))
        .expect("demo file should exist");
    assert_eq!(written, "written by demo tool provider");
}

#[test]
fn app_run_triggers_memory_consolidation_after_threshold() {
    let dir = temp_dir("memory-threshold");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");

    for _ in 0..2 {
        let _ = app
            .handle_cli_message("cli:local", "remember this")
            .expect("message should be handled");
    }

    assert!(
        dir.join("memories")
            .join("cli_local")
            .join("MEMORY.md")
            .exists()
    );
    assert!(
        dir.join("memories")
            .join("cli_local")
            .join("HISTORY.md")
            .exists()
    );
}

#[test]
fn dispatcher_skeleton_drains_outbound_queue_into_dispatch_records() {
    let dir = temp_dir("dispatch");
    let mut app = NanobotApp::new(Config::default(), Box::new(MessageToolProvider), &dir)
        .expect("app should build");

    let _ = app
        .handle_cli_message("qq:user-9", "queue a reply")
        .expect("message should be handled");

    let dispatches = app
        .dispatch_outbound_once()
        .expect("dispatcher should succeed");
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].channel, "qq");
    assert_eq!(dispatches[0].rendered, "queued reply");
    assert!(dispatches[0].delivery.starts_with("skipped:"));
}

#[test]
fn dispatcher_sends_qq_messages_via_python_bridge_once_runtime_is_started() {
    with_bridge_source(
        r#"
import json
import os
import threading

class QQBotBridge:
    def __init__(self, app_id, secret):
        self._stop = threading.Event()

    def start(self, inbound_sink):
        self._stop.wait()

    def stop(self):
        self._stop.set()

    def send(self, chat_id, content, metadata):
        with open(os.environ["NANOBOT_QQ_SEND_CAPTURE"], "w", encoding="utf-8") as fh:
            json.dump(
                {"chat_id": chat_id, "content": content, "metadata": metadata},
                fh,
                sort_keys=True,
            )
"#,
        || {
            let capture_path = temp_dir("dispatch-send").join("send.json");
            unsafe {
                std::env::set_var("NANOBOT_QQ_SEND_CAPTURE", &capture_path);
            }

            let mut config = Config::default();
            config.channels.qq = QQConfig {
                enabled: true,
                app_id: "10001".to_string(),
                secret: "secret".to_string(),
                allow_from: vec!["user-9".to_string()],
                ..QQConfig::default()
            };
            let dir = temp_dir("dispatch-send");
            let mut app =
                NanobotApp::new(config, Box::new(MessageToolProvider), &dir).expect("app should build");
            let handle = app
                .start_channel_runtimes()
                .expect("runtime should start")
                .pop()
                .expect("qq runtime should exist");

            let _ = app
                .handle_cli_message("qq:openid-9", "queue a reply")
                .expect("message should be handled");
            let dispatches = app
                .dispatch_outbound_once()
                .expect("dispatcher should succeed");

            handle.stop();
            handle.join().expect("runtime should join");
            unsafe {
                std::env::remove_var("NANOBOT_QQ_SEND_CAPTURE");
            }

            assert_eq!(dispatches.len(), 1);
            assert_eq!(dispatches[0].delivery, "sent");
            let rendered = fs::read_to_string(capture_path).expect("capture file should exist");
            assert!(rendered.contains("\"chat_id\": \"openid-9\""));
            assert!(rendered.contains("\"content\": \"queued reply\""));
        },
    );
}

#[test]
fn start_channel_runtimes_returns_channel_error_when_python_bridge_init_fails() {
    with_bridge_source(
        r#"
class QQBotBridge:
    def __init__(self, app_id, secret):
        raise RuntimeError("bridge init failed")
"#,
        || {
            let mut config = Config::default();
            config.channels.qq = QQConfig {
                enabled: true,
                app_id: "10001".to_string(),
                secret: "secret".to_string(),
                allow_from: vec!["user-9".to_string()],
                ..QQConfig::default()
            };
            let dir = temp_dir("runtime-error");
            let app =
                NanobotApp::new(config, Box::new(StaticProvider::default()), &dir).expect("app should build");

            match app.start_channel_runtimes() {
                Ok(_) => panic!("runtime should fail"),
                Err(crate::AppError::Channel(error)) => {
                    assert!(error.contains("bridge init failed"));
                }
                Err(other) => panic!("unexpected error: {other}"),
            }
        },
    );
}

#[test]
fn background_pump_emits_heartbeat_and_cron_records() {
    let dir = temp_dir("background");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");
    app.schedule_cron_job("digest", "send-digest", 5, 5)
        .expect("cron job should register");

    let records = app
        .pump_background_once(30)
        .expect("background pump should work");

    assert!(records.iter().any(|record| record.chat_id == "heartbeat"));
    assert!(records.iter().any(|record| record.chat_id == "cron:digest"));
    assert!(
        records
            .iter()
            .any(|record| record.rendered.contains("payload=send-digest"))
    );
}

#[test]
fn background_loop_collects_records_across_ticks() {
    let dir = temp_dir("background-loop");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");
    app.schedule_cron_job("digest", "send-digest", 10, 10)
        .expect("cron job should register");

    let records = app
        .run_background_loop(0, 10, 4)
        .expect("background loop should work");

    assert!(
        records
            .iter()
            .any(|record| record.rendered.contains("heartbeat"))
    );
    assert!(
        records
            .iter()
            .any(|record| record.rendered.contains("payload=send-digest"))
    );
}

#[test]
fn background_worker_runs_ticks_in_thread() {
    let dir = temp_dir("background-worker");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");
    app.schedule_cron_job("digest", "send-digest", 10, 10)
        .expect("cron job should register");

    let shared = app.into_shared();
    let handle = NanobotApp::spawn_background_worker(shared, 0, 10, 0, 4);
    let records = handle.join().expect("worker thread should join");

    assert!(records.iter().any(|record| record.chat_id == "heartbeat"));
    assert!(records.iter().any(|record| record.chat_id == "cron:digest"));
    assert!(
        records
            .iter()
            .any(|record| record.rendered.contains("payload=send-digest"))
    );
}

#[test]
fn app_processes_inbound_messages_from_bus() {
    let dir = temp_dir("inbound");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "processed inbound")),
        &dir,
    )
    .expect("app should build");

    app.bus
        .publish_inbound(nanobot_bus::InboundMessage {
            channel: "feishu".to_string(),
            sender_id: "ou_1".to_string(),
            chat_id: "oc_9".to_string(),
            content: "hello from inbound".to_string(),
            media: Vec::new(),
            metadata: std::collections::HashMap::new(),
            session_key_override: None,
        })
        .expect("inbound should publish");

    let processed = app
        .process_inbound_once()
        .expect("inbound processing should succeed");

    assert_eq!(processed, 1);
    let session = app
        .session_manager
        .load("feishu:oc_9")
        .expect("session should load")
        .expect("session should exist");
    assert_eq!(session.messages.len(), 2);
    assert_eq!(
        session.messages[1].content,
        "processed inbound: hello from inbound"
    );
}

#[test]
fn schedule_cron_job_returns_duplicate_name_error() {
    let dir = temp_dir("cron-duplicate");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");

    app.schedule_cron_job("digest", "send-digest", 5, 5)
        .expect("first cron job should register");

    assert!(matches!(
        app.schedule_cron_job("digest", "send-digest-again", 10, 10),
        Err(crate::AppError::Cron(CronError::DuplicateJobName(name))) if name == "digest"
    ));
}

struct MessageToolProvider;

impl LlmProvider for MessageToolProvider {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
        let already_has_tool = request
            .messages
            .iter()
            .any(|message| message.role == "tool" && message.content.contains("queued message"));
        if already_has_tool {
            return Ok(LlmResponse {
                content: Some("done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            });
        }

        Ok(LlmResponse {
            content: None,
            tool_calls: vec![ToolCallRequest {
                id: "msg-1".to_string(),
                name: "message".to_string(),
                arguments: json!({"content": "queued reply"}),
            }],
            finish_reason: "tool_calls".to_string(),
        })
    }

    fn default_model(&self) -> &str {
        "offline/message-tool"
    }
}

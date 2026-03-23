use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nanobot_channels::Channel;
use nanobot_config::Config;
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

#[test]
fn app_builds_with_no_channels_configured() {
    let app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::default()),
        temp_dir("no-channels"),
    )
    .expect("app should build");

    assert!(app.enabled_channel_names().is_empty());
}

#[test]
fn enabled_channel_names_is_empty_by_default() {
    let app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::default()),
        temp_dir("enabled-channel-names"),
    )
    .expect("app should build");

    assert_eq!(app.enabled_channel_names(), Vec::<&'static str>::new());
}

#[test]
fn status_summary_reports_no_channels_by_default() {
    let app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        temp_dir("status"),
    )
    .expect("app should build");

    let status = app.status_summary();
    assert!(status.contains("channels=none"));
    assert!(status.contains("offline/test"));
}

#[test]
fn start_channel_runtimes_returns_empty_when_no_channels_registered() {
    let app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::default()),
        temp_dir("channel-runtimes"),
    )
    .expect("app should build");

    let handles = app
        .start_channel_runtimes()
        .expect("starting runtimes should succeed");

    assert!(handles.is_empty());
}

#[test]
fn dispatch_outbound_uses_registered_generic_channel_from_config() {
    let dir = temp_dir("registered-generic-channel");
    let deliveries = Arc::new(Mutex::new(Vec::new()));
    let config = Config::from_json_str(
        r#"{
            "channels": [
                {
                    "kind": "generic",
                    "enabled": true
                }
            ]
        }"#,
    )
    .expect("config should parse");
    let mut app = NanobotApp::new_with_channels(
        config,
        Box::new(MessageToolProvider),
        &dir,
        vec![Box::new(FakeChannel::new("generic", deliveries.clone()))],
    )
    .expect("app should build");

    let _ = app
        .handle_cli_message("generic:user-9", "queue a reply")
        .expect("message should be handled");
    let dispatches = app
        .dispatch_outbound_once()
        .expect("dispatcher should succeed");

    assert_eq!(app.enabled_channel_names(), vec!["generic"]);
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].delivery, "sent");
    let sent = deliveries.lock().expect("deliveries lock");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "generic");
    assert_eq!(sent[0].chat_id, "user-9");
    assert_eq!(sent[0].content, "queued reply");
}

#[test]
fn from_config_with_channels_uses_registered_generic_channel_from_config() {
    let dir = temp_dir("from-config-registered-generic-channel");
    let deliveries = Arc::new(Mutex::new(Vec::new()));
    let config = Config::from_json_str(
        r#"{
            "channels": [
                {
                    "kind": "generic",
                    "enabled": true
                }
            ]
        }"#,
    )
    .expect("config should parse");
    let mut app = NanobotApp::from_config_with_channels(
        config,
        &dir,
        vec![Box::new(FakeChannel::new("generic", deliveries.clone()))],
    )
    .expect("app should build");
    app.bus
        .publish_outbound(nanobot_bus::OutboundMessage {
            channel: "generic".to_string(),
            chat_id: "user-9".to_string(),
            content: "queued reply".to_string(),
            metadata: HashMap::new(),
            reply_to: None,
        })
        .expect("outbound should publish");
    let dispatches = app
        .dispatch_outbound_once()
        .expect("dispatcher should succeed");

    assert_eq!(app.enabled_channel_names(), vec!["generic"]);
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].delivery, "sent");
    let sent = deliveries.lock().expect("deliveries lock");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "generic");
    assert_eq!(sent[0].chat_id, "user-9");
    assert_eq!(sent[0].content, "queued reply");
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
fn from_config_builds_builtin_telegram_channel() {
    let config = Config::from_json_str(
        r#"{
            "channels": [
                {
                    "kind": "telegram",
                    "enabled": true,
                    "allowFrom": ["42"],
                    "settings": {
                        "botToken": "123:test"
                    }
                }
            ]
        }"#,
    )
    .expect("config should parse");

    let app = NanobotApp::from_config(config, temp_dir("telegram-channel"))
        .expect("app should build with telegram channel");

    assert_eq!(app.enabled_channel_names(), vec!["telegram"]);
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
fn cli_message_run_publishes_outbound_messages_from_tools() {
    let dir = temp_dir("outbound");
    let mut app = NanobotApp::new(Config::default(), Box::new(MessageToolProvider), &dir)
        .expect("app should build");

    let _ = app
        .handle_cli_message("generic:user-9", "queue a reply")
        .expect("message should be handled");

    let published = app.take_outbound_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].channel, "generic");
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
fn app_run_compacts_session_without_writing_memory_files() {
    let dir = temp_dir("session-compact");
    let mut app = NanobotApp::new(
        Config::default(),
        Box::new(StaticProvider::new("offline/test", "assistant")),
        &dir,
    )
    .expect("app should build");

    for turn in 0..101 {
        let _ = app
            .handle_cli_message("cli:local", &format!("remember this {turn}"))
            .expect("message should be handled");
    }

    let session = app
        .session_manager
        .load("cli:local")
        .expect("session should load")
        .expect("session should exist");
    assert!(
        session.messages.len() < 202,
        "session should be compacted instead of keeping every original message"
    );
    assert!(session.messages.iter().any(|message| {
        message.role == "system"
            && message.metadata.get("kind").map(String::as_str) == Some("compact_summary")
    }));
    assert!(
        !dir.join("memories").join("cli_local").exists(),
        "legacy memory files should not be created"
    );
}

#[test]
fn dispatcher_skips_unregistered_channels_as_unsupported() {
    let dir = temp_dir("dispatch");
    let mut app = NanobotApp::new(Config::default(), Box::new(MessageToolProvider), &dir)
        .expect("app should build");

    let _ = app
        .handle_cli_message("generic:user-9", "queue a reply")
        .expect("message should be handled");

    let dispatches = app
        .dispatch_outbound_once()
        .expect("dispatcher should succeed");
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].channel, "generic");
    assert_eq!(dispatches[0].rendered, "queued reply");
    assert_eq!(dispatches[0].delivery, "skipped:unsupported_channel");
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
            channel: "generic".to_string(),
            sender_id: "sender-1".to_string(),
            chat_id: "chat-9".to_string(),
            content: "hello from inbound".to_string(),
            media: Vec::new(),
            metadata: HashMap::new(),
            session_key_override: None,
        })
        .expect("inbound should publish");

    let processed = app
        .process_inbound_once()
        .expect("inbound processing should succeed");

    assert_eq!(processed, 1);
    let session = app
        .session_manager
        .load("generic:chat-9")
        .expect("session should load")
        .expect("session should exist");
    assert_eq!(session.messages.len(), 2);
    assert_eq!(
        session.messages[1].content.as_deref(),
        Some("processed inbound: hello from inbound")
    );
}

#[test]
fn inbound_stop_response_is_published_back_to_source_channel() {
    let dir = temp_dir("inbound-stop-response");
    let deliveries = Arc::new(Mutex::new(Vec::new()));
    let config = Config::from_json_str(
        r#"{
            "channels": [
                {
                    "kind": "generic",
                    "enabled": true
                }
            ]
        }"#,
    )
    .expect("config should parse");
    let mut app = NanobotApp::new_with_channels(
        config,
        Box::new(StaticProvider::new("offline/test", "processed inbound")),
        &dir,
        vec![Box::new(FakeChannel::new("generic", deliveries.clone()))],
    )
    .expect("app should build");

    app.bus
        .publish_inbound(nanobot_bus::InboundMessage {
            channel: "generic".to_string(),
            sender_id: "sender-1".to_string(),
            chat_id: "chat-9".to_string(),
            content: "hello from inbound".to_string(),
            media: Vec::new(),
            metadata: HashMap::new(),
            session_key_override: None,
        })
        .expect("inbound should publish");

    let processed = app
        .process_inbound_once()
        .expect("inbound processing should succeed");
    assert_eq!(processed, 1);

    let dispatches = app
        .dispatch_outbound_once()
        .expect("dispatcher should succeed");
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].channel, "generic");
    assert_eq!(dispatches[0].chat_id, "chat-9");
    assert_eq!(
        dispatches[0].rendered,
        "processed inbound: hello from inbound"
    );
    assert_eq!(dispatches[0].delivery, "sent");

    let sent = deliveries.lock().expect("deliveries lock");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].content, "processed inbound: hello from inbound");
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

struct FakeChannel {
    name: &'static str,
    allow_from: Vec<String>,
    deliveries: Arc<Mutex<Vec<nanobot_channels::OutboundMessage>>>,
}

impl FakeChannel {
    fn new(
        name: &'static str,
        deliveries: Arc<Mutex<Vec<nanobot_channels::OutboundMessage>>>,
    ) -> Self {
        Self {
            name,
            allow_from: vec!["*".to_string()],
            deliveries,
        }
    }
}

impl Channel for FakeChannel {
    fn name(&self) -> &'static str {
        self.name
    }

    fn allow_from(&self) -> &[String] {
        &self.allow_from
    }

    fn send(
        &self,
        msg: &nanobot_channels::OutboundMessage,
    ) -> Result<(), nanobot_channels::ChannelError> {
        self.deliveries
            .lock()
            .expect("deliveries lock")
            .push(msg.clone());
        Ok(())
    }
}

impl LlmProvider for MessageToolProvider {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
        let already_has_tool = request.messages.iter().any(|message| {
            message.role == "tool"
                && message
                    .content
                    .clone()
                    .unwrap_or_default()
                    .contains("queued message")
        });
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

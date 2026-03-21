use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nanobot_bus::{InboundMessage, MessageBus, OutboundMessage};
use nanobot_channel_feishu::FeishuChannel;
use nanobot_channel_qq::QQChannel;
use nanobot_channels::{Channel, ChannelRuntimeHandle};
use nanobot_config::Config;
use nanobot_core::{AgentError, AgentLoop};
use nanobot_cron::CronService;
use nanobot_heartbeat::HeartbeatService;
use nanobot_provider::{LlmProvider, build_provider_from_config};
use nanobot_session::{SessionError, SessionManager};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("agent error: {0}")]
    Agent(#[from] AgentError),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
}

pub struct NanobotApp {
    provider: Box<dyn LlmProvider>,
    agent_loop: AgentLoop,
    session_manager: SessionManager,
    bus: MessageBus,
    feishu_channel: Option<FeishuChannel>,
    qq_channel: Option<QQChannel>,
    channels: Vec<Box<dyn Channel>>,
    cron: CronService,
    heartbeat: HeartbeatService,
    provider_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRecord {
    pub channel: String,
    pub chat_id: String,
    pub rendered: String,
    pub delivery: String,
}

pub struct BackgroundWorkerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<Vec<DispatchRecord>>>,
}

impl BackgroundWorkerHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn join(mut self) -> std::thread::Result<Vec<DispatchRecord>> {
        self.join.take().expect("join handle").join()
    }
}

impl NanobotApp {
    pub fn from_config(config: Config, workspace: impl Into<PathBuf>) -> Result<Self, AppError> {
        let provider = build_provider_from_config(&config);
        Self::new(config, provider, workspace)
    }

    pub fn new(
        config: Config,
        provider: Box<dyn LlmProvider>,
        workspace: impl Into<PathBuf>,
    ) -> Result<Self, AppError> {
        let workspace = workspace.into();
        let session_manager = SessionManager::new(workspace.join("sessions"))?;
        let bus = MessageBus::new();
        let mut channels: Vec<Box<dyn Channel>> = Vec::new();
        let feishu_channel = if config.channels.feishu.enabled {
            Some(FeishuChannel::new(config.channels.feishu.clone()))
        } else {
            None
        };
        let qq_channel = if config.channels.qq.enabled {
            Some(QQChannel::new(config.channels.qq.clone()))
        } else {
            None
        };
        if config.channels.feishu.enabled {
            channels.push(Box::new(FeishuChannel::new(config.channels.feishu.clone())));
        }
        if config.channels.qq.enabled {
            channels.push(Box::new(QQChannel::new(config.channels.qq.clone())));
        }
        let provider_model = provider.default_model().to_string();
        let mut agent_loop = AgentLoop::new(provider_model.clone());
        agent_loop.set_workspace_root(&workspace);
        Ok(Self {
            provider,
            agent_loop,
            session_manager,
            bus,
            feishu_channel,
            qq_channel,
            channels,
            cron: CronService::default(),
            heartbeat: HeartbeatService::new(30),
            provider_model,
        })
    }

    pub fn enabled_channel_names(&self) -> Vec<&'static str> {
        self.channels.iter().map(|channel| channel.name()).collect()
    }

    pub fn handle_cli_message(
        &mut self,
        session_key: &str,
        user_input: &str,
    ) -> Result<Option<String>, AppError> {
        let mut session = self.session_manager.load_or_create(session_key)?;
        let (channel, chat_id) = split_session_key(session_key);
        self.agent_loop.set_message_target(channel, chat_id);
        let response =
            self.agent_loop
                .run_once(self.provider.as_ref(), &mut session, user_input)?;
        for outbound in self.agent_loop.take_outbound_messages() {
            self.bus
                .publish_outbound(outbound)
                .map_err(|error| AgentError::Tool(error.to_string()))?;
        }
        let _ = self.session_manager.maybe_consolidate(&mut session, 4)?;
        self.session_manager.save(&session)?;
        Ok(response)
    }

    pub fn handle_inbound_message(
        &mut self,
        inbound: InboundMessage,
    ) -> Result<Option<String>, AppError> {
        let session_key = inbound.session_key();
        let mut session = self.session_manager.load_or_create(&session_key)?;
        let mut metadata = inbound.metadata.clone();
        metadata.insert("sender_id".to_string(), inbound.sender_id.clone());
        metadata.insert("channel".to_string(), inbound.channel.clone());
        metadata.insert("chat_id".to_string(), inbound.chat_id.clone());
        session.metadata.extend(metadata);
        self.agent_loop
            .set_message_target(inbound.channel.clone(), inbound.chat_id.clone());
        let response =
            self.agent_loop
                .run_once(self.provider.as_ref(), &mut session, &inbound.content)?;
        for outbound in self.agent_loop.take_outbound_messages() {
            self.bus
                .publish_outbound(outbound)
                .map_err(|error| AgentError::Tool(error.to_string()))?;
        }
        let _ = self.session_manager.maybe_consolidate(&mut session, 4)?;
        self.session_manager.save(&session)?;
        Ok(response)
    }

    pub fn process_inbound_once(&mut self) -> Result<usize, AppError> {
        let mut processed = 0;
        while let Some(inbound) = self.bus.try_consume_inbound() {
            let _ = self.handle_inbound_message(inbound)?;
            processed += 1;
        }
        Ok(processed)
    }

    pub fn take_outbound_messages(&mut self) -> Vec<OutboundMessage> {
        let mut messages = Vec::new();
        while let Some(message) = self.bus.try_consume_outbound() {
            messages.push(message);
        }
        messages
    }

    pub fn dispatch_outbound_once(&mut self) -> Result<Vec<DispatchRecord>, AppError> {
        let outbound = self.take_outbound_messages();
        let mut records = Vec::new();
        for msg in outbound {
            let rendered = self.render_outbound(&msg);
            let delivery = self.deliver_outbound(&msg);
            records.push(DispatchRecord {
                channel: msg.channel,
                chat_id: msg.chat_id,
                rendered,
                delivery,
            });
        }
        Ok(records)
    }

    pub fn schedule_cron_job(
        &mut self,
        name: impl Into<String>,
        interval_ticks: u64,
        next_tick: u64,
    ) {
        let _ = self.cron.add_job(name, interval_ticks, next_tick);
    }

    pub fn pump_background_once(&mut self, now_tick: u64) -> Result<Vec<DispatchRecord>, AppError> {
        let mut records = self.dispatch_outbound_once()?;
        if self.heartbeat.is_due(now_tick) {
            self.heartbeat.mark_sent(now_tick);
            records.push(DispatchRecord {
                channel: "system".to_string(),
                chat_id: "heartbeat".to_string(),
                rendered: format!("heartbeat tick={now_tick}"),
                delivery: "local".to_string(),
            });
        }
        for job in self.cron.tick(now_tick) {
            records.push(DispatchRecord {
                channel: "system".to_string(),
                chat_id: format!("cron:{job}"),
                rendered: format!("cron job due: {job}"),
                delivery: "local".to_string(),
            });
        }
        Ok(records)
    }

    pub fn run_background_loop(
        &mut self,
        start_tick: u64,
        tick_step: u64,
        iterations: usize,
    ) -> Result<Vec<DispatchRecord>, AppError> {
        let mut out = Vec::new();
        for index in 0..iterations {
            let tick = start_tick + tick_step.saturating_mul(index as u64);
            out.extend(self.pump_background_once(tick)?);
        }
        Ok(out)
    }

    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    pub fn start_channel_runtimes(&self) -> Vec<ChannelRuntimeHandle> {
        let inbound_tx = self.bus.inbound_publisher();
        let mut handles = Vec::new();
        if let Some(channel) = &self.feishu_channel {
            if let Some(handle) = channel.spawn_inbound_runtime(inbound_tx.clone()) {
                handles.push(handle);
            }
        }
        if let Some(channel) = &self.qq_channel {
            if let Some(handle) = channel.spawn_inbound_runtime(inbound_tx.clone()) {
                handles.push(handle);
            }
        }
        handles
    }

    pub fn spawn_background_worker(
        app: Arc<Mutex<Self>>,
        start_tick: u64,
        tick_step: u64,
        interval_ms: u64,
        max_iterations: usize,
    ) -> BackgroundWorkerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        let join = thread::spawn(move || {
            let mut records = Vec::new();
            for index in 0..max_iterations {
                if stop_worker.load(Ordering::SeqCst) {
                    break;
                }
                let tick = start_tick + tick_step.saturating_mul(index as u64);
                let mut app = app.lock().expect("background app lock");
                if let Ok(mut batch) = app.pump_background_once(tick) {
                    records.append(&mut batch);
                }
                drop(app);
                if interval_ms > 0 {
                    thread::sleep(Duration::from_millis(interval_ms));
                }
            }
            records
        });
        BackgroundWorkerHandle {
            stop,
            join: Some(join),
        }
    }

    pub fn status_summary(&self) -> String {
        let channels = if self.channels.is_empty() {
            "none".to_string()
        } else {
            self.enabled_channel_names().join(", ")
        };
        format!(
            "model={} channels={} heartbeat_due_now={} scheduled_jobs={}",
            self.provider_model,
            channels,
            self.heartbeat.is_due(30),
            self.cron.job_count()
        )
    }
}

fn split_session_key(session_key: &str) -> (String, String) {
    match session_key.split_once(':') {
        Some((channel, chat_id)) => (channel.to_string(), chat_id.to_string()),
        None => ("cli".to_string(), session_key.to_string()),
    }
}

impl NanobotApp {
    fn render_outbound(&self, msg: &OutboundMessage) -> String {
        match msg.channel.as_str() {
            "feishu" => self
                .channels
                .iter()
                .find(|channel| channel.name() == "feishu")
                .map(|_| msg.content.clone())
                .unwrap_or_else(|| msg.content.clone()),
            "qq" => self
                .channels
                .iter()
                .find(|channel| channel.name() == "qq")
                .map(|_| msg.content.clone())
                .unwrap_or_else(|| msg.content.clone()),
            _ => msg.content.clone(),
        }
    }

    fn deliver_outbound(&self, msg: &OutboundMessage) -> String {
        match msg.channel.as_str() {
            "feishu" => {
                if let Some(channel) = &self.feishu_channel {
                    match channel.fetch_access_token_via_curl() {
                        Ok(token) => match channel.send_via_curl(&token, &msg.chat_id, msg) {
                            Ok(_) => "sent".to_string(),
                            Err(error) => format!("send_failed:{error}"),
                        },
                        Err(error) => format!("token_failed:{error}"),
                    }
                } else {
                    "skipped:channel_disabled".to_string()
                }
            }
            "qq" => {
                if let Some(channel) = &self.qq_channel {
                    match channel.fetch_access_token_via_curl() {
                        Ok(token) => match channel.send_via_curl(&token, &msg.chat_id, msg) {
                            Ok(_) => "sent".to_string(),
                            Err(error) => format!("send_failed:{error}"),
                        },
                        Err(error) => format!("token_failed:{error}"),
                    }
                } else {
                    "skipped:channel_disabled".to_string()
                }
            }
            _ => "skipped:unsupported_channel".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nanobot_config::{Config, FeishuConfig, QQConfig};
    use nanobot_provider::{
        ChatRequest, LlmProvider, LlmResponse, ProviderError, StaticProvider, ToolCallRequest,
    };
    use serde_json::json;

    use crate::NanobotApp;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nanobot-app-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir should exist");
        dir
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
                    "openrouter": { "apiKey": "sk-or-v1-123" }
                },
                "agents": {
                    "defaults": {
                        "model": "gpt-4o-mini",
                        "provider": "openrouter"
                    }
                }
            }"#,
        )
        .expect("config should parse");

        let app = NanobotApp::from_config(config, temp_dir("provider")).expect("app should build");
        assert!(app.status_summary().contains("openrouter/auto"));
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
        let config = Config::from_json_str(
            r#"{
                "agents": {
                    "defaults": {
                        "model": "offline/tool-calling-demo",
                        "provider": "demo_tool_calling"
                    }
                }
            }"#,
        )
        .expect("config should parse");

        let mut app = NanobotApp::from_config(config, &dir).expect("app should build");
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
    fn background_pump_emits_heartbeat_and_cron_records() {
        let dir = temp_dir("background");
        let mut app = NanobotApp::new(
            Config::default(),
            Box::new(StaticProvider::new("offline/test", "assistant")),
            &dir,
        )
        .expect("app should build");
        app.schedule_cron_job("digest", 5, 5);

        let records = app
            .pump_background_once(30)
            .expect("background pump should work");

        assert!(records.iter().any(|record| record.chat_id == "heartbeat"));
        assert!(records.iter().any(|record| record.chat_id == "cron:digest"));
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
        app.schedule_cron_job("digest", 10, 10);

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
                .any(|record| record.rendered.contains("cron job due: digest"))
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
        app.schedule_cron_job("digest", 10, 10);

        let shared = app.into_shared();
        let handle = NanobotApp::spawn_background_worker(shared, 0, 10, 0, 4);
        let records = handle.join().expect("worker thread should join");

        assert!(records.iter().any(|record| record.chat_id == "heartbeat"));
        assert!(records.iter().any(|record| record.chat_id == "cron:digest"));
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

    struct MessageToolProvider;

    impl LlmProvider for MessageToolProvider {
        fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
            let already_has_tool = request.messages.iter().any(|message| {
                message.role == "tool" && message.content.contains("queued message")
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
}

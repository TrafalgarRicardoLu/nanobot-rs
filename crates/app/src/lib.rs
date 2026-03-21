use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nanobot_bus::{InboundMessage, MessageBus, OutboundMessage};
use nanobot_channels::{Channel, ChannelRuntimeHandle};
use nanobot_config::Config;
use nanobot_core::{AgentError, AgentLoop};
use nanobot_cron::{CronError, CronService};
use nanobot_heartbeat::HeartbeatService;
use nanobot_provider::{build_provider_from_config, LlmProvider};
use nanobot_session::{SessionError, SessionManager};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("agent error: {0}")]
    Agent(#[from] AgentError),
    #[error("channel error: {0}")]
    Channel(String),
    #[error("cron error: {0}")]
    Cron(#[from] CronError),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
}

pub struct NanobotApp {
    provider: Box<dyn LlmProvider>,
    agent_loop: AgentLoop,
    session_manager: SessionManager,
    bus: MessageBus,
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

    pub fn from_config_with_channels(
        config: Config,
        workspace: impl Into<PathBuf>,
        channels: Vec<Box<dyn Channel>>,
    ) -> Result<Self, AppError> {
        let provider = build_provider_from_config(&config);
        Self::new_with_channels(config, provider, workspace, channels)
    }

    pub fn new(
        config: Config,
        provider: Box<dyn LlmProvider>,
        workspace: impl Into<PathBuf>,
    ) -> Result<Self, AppError> {
        Self::new_with_channels(config, provider, workspace, Vec::new())
    }

    pub fn new_with_channels(
        config: Config,
        provider: Box<dyn LlmProvider>,
        workspace: impl Into<PathBuf>,
        channels: Vec<Box<dyn Channel>>,
    ) -> Result<Self, AppError> {
        let workspace = workspace.into();
        let session_manager = SessionManager::new(workspace.join("sessions"))?;
        let bus = MessageBus::new();
        let enabled_channel_kinds: HashSet<_> = config
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .map(|channel| channel.kind.as_str())
            .collect();
        let channels = channels
            .into_iter()
            .filter(|channel| enabled_channel_kinds.contains(channel.name()))
            .collect();
        let provider_model = provider.default_model().to_string();
        let mut agent_loop = AgentLoop::new(provider_model.clone());
        agent_loop.set_workspace_root(&workspace);
        Ok(Self {
            provider,
            agent_loop,
            session_manager,
            bus,
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
        payload: impl Into<String>,
        interval_ticks: u64,
        next_tick: u64,
    ) -> Result<(), AppError> {
        self.cron
            .add_job(name, payload, interval_ticks, next_tick)
            .map_err(AppError::from)
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
                chat_id: format!("cron:{}", job.name),
                rendered: format!("cron job due: {} payload={}", job.name, job.payload),
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

    pub fn start_channel_runtimes(&self) -> Result<Vec<ChannelRuntimeHandle>, AppError> {
        let inbound_tx = self.bus.inbound_publisher();
        let mut handles = Vec::new();
        for channel in &self.channels {
            if let Some(handle) = channel.spawn_inbound_runtime(inbound_tx.clone()) {
                handles.push(handle);
            }
        }
        Ok(handles)
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
        msg.content.clone()
    }

    fn deliver_outbound(&self, msg: &OutboundMessage) -> String {
        match self
            .channels
            .iter()
            .find(|channel| channel.name() == msg.channel)
        {
            Some(channel) => match channel.send(msg) {
                Ok(_) => "sent".to_string(),
                Err(error) => format!("send_failed:{error}"),
            },
            None => "skipped:unsupported_channel".to_string(),
        }
    }
}

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use log::info;
use nanobot_bus::{InboundMessage, MessageBus, OutboundMessage};
use nanobot_channels::Channel;
use nanobot_config::Config;
use nanobot_core::{AgentError, AgentLoop};
use nanobot_cron::CronService;
use nanobot_heartbeat::HeartbeatService;
use nanobot_provider::{LlmProvider, build_provider_from_config};
use nanobot_session::SessionManager;

use crate::{AppError, DispatchRecord, build_builtin_channels, split_session_key};

pub struct NanobotApp {
    pub(crate) provider: Box<dyn LlmProvider>,
    pub(crate) agent_loop: AgentLoop,
    pub(crate) session_manager: SessionManager,
    pub(crate) bus: MessageBus,
    pub(crate) channels: Vec<Box<dyn Channel>>,
    pub(crate) cron: CronService,
    pub(crate) heartbeat: HeartbeatService,
    pub(crate) provider_model: String,
}

impl NanobotApp {
    fn heartbeat_inbound(now_tick: u64) -> InboundMessage {
        InboundMessage {
            channel: "system".to_string(),
            sender_id: "heartbeat".to_string(),
            chat_id: "heartbeat".to_string(),
            content: format!("heartbeat tick={now_tick}"),
            media: Vec::new(),
            metadata: HashMap::from([
                ("source".to_string(), "heartbeat".to_string()),
                ("scheduled_tick".to_string(), now_tick.to_string()),
            ]),
            session_key_override: Some(format!("system:heartbeat:{now_tick}")),
        }
    }

    fn background_dispatch_record(msg: &InboundMessage) -> DispatchRecord {
        let source = msg
            .metadata
            .get("source")
            .map(String::as_str)
            .unwrap_or("background");
        let rendered = if source == "cron" {
            let job_name = msg
                .metadata
                .get("job_name")
                .map(String::as_str)
                .unwrap_or(msg.chat_id.as_str());
            let session = msg.session_key_override.as_deref().unwrap_or_default();
            format!(
                "queued cron inbound: {} session={} payload={}",
                job_name, session, msg.content
            )
        } else {
            format!("queued {source} inbound: {}", msg.content)
        };

        let chat_id = if source == "cron" {
            let job_name = msg
                .metadata
                .get("job_name")
                .map(String::as_str)
                .unwrap_or(msg.chat_id.as_str());
            format!("cron:{job_name}")
        } else {
            msg.chat_id.clone()
        };

        DispatchRecord {
            channel: msg.channel.clone(),
            chat_id,
            rendered,
            delivery: "local".to_string(),
        }
    }

    pub fn from_config(config: Config, workspace: impl Into<PathBuf>) -> Result<Self, AppError> {
        let provider = build_provider_from_config(&config);
        let channels = build_builtin_channels(&config)?;
        Self::new_with_channels(config, provider, workspace, channels)
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
        info!("app handle_cli_message session_key={session_key:?} user_input={user_input:?}");
        let mut session = self.session_manager.load_or_create(session_key)?;
        let (channel, chat_id) = split_session_key(session_key);
        self.agent_loop.set_message_target(channel, chat_id);
        let response =
            self.agent_loop
                .run_once(self.provider.as_ref(), &mut session, user_input)?;
        info!("app handle_cli_message agent_response={response:?}");
        for outbound in self.agent_loop.take_outbound_messages() {
            info!("app handle_cli_message publishing outbound={outbound:?}");
            self.bus
                .publish_outbound(outbound)
                .map_err(|error| AgentError::Tool(error.to_string()))?;
        }
        self.session_manager.save(&session)?;
        Ok(response)
    }

    pub fn handle_inbound_message(
        &mut self,
        inbound: InboundMessage,
    ) -> Result<Option<String>, AppError> {
        info!("app handle_inbound_message inbound={inbound:?}");
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
        info!("app handle_inbound_message agent_response={response:?}");
        if let Some(content) = response.clone() {
            let outbound = OutboundMessage {
                channel: inbound.channel.clone(),
                chat_id: inbound.chat_id.clone(),
                content,
                reply_to: inbound.metadata.get("telegram_message_id").cloned(),
                metadata: HashMap::new(),
            };
            info!("app handle_inbound_message publishing assistant outbound={outbound:?}");
            self.bus
                .publish_outbound(outbound)
                .map_err(|error| AgentError::Tool(error.to_string()))?;
        }
        for outbound in self.agent_loop.take_outbound_messages() {
            info!("app handle_inbound_message publishing outbound={outbound:?}");
            self.bus
                .publish_outbound(outbound)
                .map_err(|error| AgentError::Tool(error.to_string()))?;
        }
        self.session_manager.save(&session)?;
        Ok(response)
    }

    pub fn process_inbound_once(&mut self) -> Result<usize, AppError> {
        let mut processed = 0;
        while let Some(inbound) = self.bus.try_consume_inbound() {
            info!("app process_inbound_once dispatching inbound={inbound:?}");
            let _ = self.handle_inbound_message(inbound)?;
            processed += 1;
        }
        info!("app process_inbound_once processed={processed}");
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
        info!("app dispatch_outbound_once outbound_batch={outbound:?}");
        let mut records = Vec::new();
        for msg in outbound {
            let rendered = self.render_outbound(&msg);
            let delivery = self.deliver_outbound(&msg);
            info!(
                "app dispatch_outbound_once delivered msg={msg:?} rendered={rendered:?} delivery={delivery:?}"
            );
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
        session_id: impl Into<String>,
        payload: impl Into<String>,
        interval_ticks: u64,
        next_tick: u64,
    ) -> Result<(), AppError> {
        self.cron
            .add_job(name, session_id, payload, interval_ticks, next_tick)
            .map_err(AppError::from)
    }

    pub fn pump_background_once(&mut self, now_tick: u64) -> Result<Vec<DispatchRecord>, AppError> {
        let mut records = self.dispatch_outbound_once()?;
        let mut background_inbound = Vec::new();

        if let Some(now_tick) = self.heartbeat.tick(now_tick) {
            background_inbound.push(Self::heartbeat_inbound(now_tick));
        }

        for job in self.cron.tick(now_tick) {
            background_inbound.push(InboundMessage {
                channel: "system".to_string(),
                sender_id: "cron".to_string(),
                chat_id: job.name.clone(),
                content: job.payload.clone(),
                media: Vec::new(),
                metadata: HashMap::from([
                    ("source".to_string(), "cron".to_string()),
                    ("job_name".to_string(), job.name.clone()),
                    ("scheduled_tick".to_string(), now_tick.to_string()),
                ]),
                session_key_override: Some(job.session_id.clone()),
            });
        }

        for inbound in background_inbound {
            self.bus
                .publish_inbound(inbound.clone())
                .map_err(|error| AgentError::Tool(error.to_string()))?;
            records.push(Self::background_dispatch_record(&inbound));
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

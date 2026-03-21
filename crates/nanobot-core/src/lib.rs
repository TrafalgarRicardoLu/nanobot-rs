use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use nanobot_provider::{ChatMessage, ChatRequest, LlmProvider, LlmResponse, ProviderError};
use nanobot_session::{Session, SessionError};
use nanobot_tools::ToolRegistry;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("agent run cancelled")]
    Cancelled,
    #[error("agent run exceeded max steps: {0}")]
    MaxStepsExceeded(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunConfig {
    pub max_steps: usize,
    pub merge_consecutive_user: bool,
    pub drop_empty_messages: bool,
    pub emit_progress: bool,
    pub emit_tool_hints: bool,
    pub continue_after_tool_calls: bool,
}

impl Default for AgentRunConfig {
    fn default() -> Self {
        Self {
            max_steps: 5,
            merge_consecutive_user: true,
            drop_empty_messages: true,
            emit_progress: true,
            emit_tool_hints: true,
            continue_after_tool_calls: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRunStatus {
    Ready,
    Running,
    Interrupted,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    RunStarted {
        input: String,
    },
    StepStarted {
        step: usize,
    },
    ProviderResponded {
        finish_reason: String,
        tool_calls: usize,
    },
    ToolHint {
        name: String,
    },
    ToolCallStarted {
        name: String,
    },
    ToolCallFinished {
        name: String,
        result: String,
    },
    SubagentStarted {
        name: String,
        task: String,
    },
    SubagentFinished {
        name: String,
        result: String,
    },
    SkillActivated {
        name: String,
    },
    AssistantMessage {
        content: String,
    },
    RunCompleted {
        steps: usize,
    },
    RunCancelled {
        step: usize,
    },
    RunInterrupted {
        step: usize,
    },
    RunFailed {
        step: usize,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunReport {
    pub status: AgentRunStatus,
    pub steps: usize,
    pub response: Option<String>,
    pub events: Vec<AgentEvent>,
    pub tool_calls: usize,
    pub subagent_calls: usize,
    pub skill_activations: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingRunState {
    steps_completed: usize,
    pending_response: LlmResponse,
    skill_activations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub name: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    definitions: HashMap<String, SkillDefinition>,
}

impl SkillRegistry {
    pub fn register(&mut self, name: impl Into<String>, instructions: impl Into<String>) -> bool {
        let name = name.into();
        let instructions = instructions.into().trim().to_string();
        if instructions.is_empty() {
            return false;
        }
        self.definitions
            .insert(name.clone(), SkillDefinition { name, instructions });
        true
    }

    pub fn get(&self, name: &str) -> Option<&SkillDefinition> {
        self.definitions.get(name)
    }

    pub fn load_from_dir(&mut self, dir: impl AsRef<Path>) -> Result<usize, AgentError> {
        let mut loaded = 0;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let content = fs::read_to_string(&path)?;
            if self.register(stem.to_string(), content) {
                loaded += 1;
            }
        }
        Ok(loaded)
    }

    pub fn resolve_from_input(&self, input: &str) -> Vec<SkillDefinition> {
        let mut active = Vec::new();
        for token in input.split_whitespace() {
            let Some(name) = token.strip_prefix('@') else {
                continue;
            };
            if let Some(skill) = self.get(name) {
                active.push(skill.clone());
            }
        }
        active
    }
}

type SubagentHandler = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct SubagentManager {
    handlers: HashMap<String, SubagentHandler>,
}

impl std::fmt::Debug for SubagentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names = self.handlers.keys().cloned().collect::<Vec<_>>();
        names.sort();
        f.debug_struct("SubagentManager")
            .field("handlers", &names)
            .finish()
    }
}

impl SubagentManager {
    pub fn register_static(&mut self, name: impl Into<String>, response_prefix: impl Into<String>) {
        let response_prefix = response_prefix.into();
        self.handlers.insert(
            name.into(),
            Arc::new(move |task: &str| Ok(format!("{response_prefix}: {task}"))),
        );
    }

    pub fn run(&self, name: &str, task: &str) -> Result<String, String> {
        let handler = self
            .handlers
            .get(name)
            .ok_or_else(|| format!("unknown subagent: {name}"))?;
        handler(task)
    }
}

#[derive(Debug, Clone)]
pub struct AgentLoop {
    model: String,
    max_history: usize,
    tools: ToolRegistry,
    run_config: AgentRunConfig,
    cancel_requested: bool,
    interrupt_after_step: Option<usize>,
    pending_run: Option<PendingRunState>,
    subagents: SubagentManager,
    skills: SkillRegistry,
}

impl AgentLoop {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_history: 200,
            tools: ToolRegistry::with_builtin_defaults(),
            run_config: AgentRunConfig::default(),
            cancel_requested: false,
            interrupt_after_step: None,
            pending_run: None,
            subagents: SubagentManager::default(),
            skills: SkillRegistry::default(),
        }
    }

    pub fn register_tool(&mut self, name: impl Into<String>, description: impl Into<String>) {
        self.tools
            .register(nanobot_tools::ToolDefinition::new(name, description));
    }

    pub fn set_workspace_root(&mut self, root: impl AsRef<Path>) {
        self.tools.set_workspace_root(root.as_ref().to_path_buf());
    }

    pub fn set_message_target(&mut self, channel: impl Into<String>, chat_id: impl Into<String>) {
        self.tools.set_message_target(channel, chat_id);
    }

    pub fn take_outbound_messages(&mut self) -> Vec<nanobot_bus::OutboundMessage> {
        self.tools.take_outbound_messages()
    }

    pub fn set_run_config(&mut self, config: AgentRunConfig) {
        self.run_config = config;
    }

    pub fn request_cancel(&mut self) {
        self.cancel_requested = true;
    }

    pub fn clear_cancel(&mut self) {
        self.cancel_requested = false;
    }

    pub fn request_interrupt_after_step(&mut self, step: usize) {
        self.interrupt_after_step = Some(step);
    }

    pub fn clear_interrupt(&mut self) {
        self.interrupt_after_step = None;
    }

    pub fn register_subagent_static(
        &mut self,
        name: impl Into<String>,
        response_prefix: impl Into<String>,
    ) {
        self.subagents.register_static(name, response_prefix);
    }

    pub fn load_skills_from_dir(&mut self, dir: impl AsRef<Path>) -> Result<usize, AgentError> {
        self.skills.load_from_dir(dir)
    }

    pub fn resume_turn(
        &mut self,
        provider: &dyn LlmProvider,
        session: &mut Session,
    ) -> Result<AgentRunReport, AgentError> {
        let Some(pending) = self.pending_run.take() else {
            return Ok(AgentRunReport {
                status: AgentRunStatus::Ready,
                steps: 0,
                response: None,
                events: Vec::new(),
                tool_calls: 0,
                subagent_calls: 0,
                skill_activations: Vec::new(),
            });
        };

        self.continue_run(
            provider,
            session,
            pending.steps_completed,
            pending.skill_activations,
            Vec::new(),
            0,
            0,
            Some(pending.pending_response),
        )
    }

    pub fn run_turn(
        &mut self,
        provider: &dyn LlmProvider,
        session: &mut Session,
        user_input: &str,
    ) -> Result<AgentRunReport, AgentError> {
        let mut events = Vec::new();
        let input = user_input.trim();
        events.push(AgentEvent::RunStarted {
            input: input.to_string(),
        });

        if self.cancel_requested {
            events.push(AgentEvent::RunCancelled { step: 0 });
            return Ok(AgentRunReport {
                status: AgentRunStatus::Cancelled,
                steps: 0,
                response: None,
                events,
                tool_calls: 0,
                subagent_calls: 0,
                skill_activations: Vec::new(),
            });
        }

        if self.run_config.drop_empty_messages && input.is_empty() {
            events.push(AgentEvent::RunCompleted { steps: 0 });
            return Ok(AgentRunReport {
                status: AgentRunStatus::Completed,
                steps: 0,
                response: None,
                events,
                tool_calls: 0,
                subagent_calls: 0,
                skill_activations: Vec::new(),
            });
        }

        let active_skills = self.skills.resolve_from_input(input);
        let skill_activations = active_skills
            .iter()
            .map(|skill| skill.name.clone())
            .collect::<Vec<_>>();
        for skill in &active_skills {
            events.push(AgentEvent::SkillActivated {
                name: skill.name.clone(),
            });
        }

        self.persist_user_message(session, input);

        self.continue_run(provider, session, 0, skill_activations, events, 0, 0, None)
    }

    fn continue_run(
        &mut self,
        provider: &dyn LlmProvider,
        session: &mut Session,
        mut steps: usize,
        skill_activations: Vec<String>,
        mut events: Vec<AgentEvent>,
        mut tool_calls: usize,
        mut subagent_calls: usize,
        mut pending_response: Option<LlmResponse>,
    ) -> Result<AgentRunReport, AgentError> {
        loop {
            if self.cancel_requested {
                events.push(AgentEvent::RunCancelled { step: steps });
                return Ok(AgentRunReport {
                    status: AgentRunStatus::Cancelled,
                    steps,
                    response: None,
                    events,
                    tool_calls,
                    subagent_calls,
                    skill_activations,
                });
            }

            if pending_response.is_none() && steps >= self.run_config.max_steps {
                events.push(AgentEvent::RunFailed {
                    step: steps,
                    message: format!("max steps exceeded: {}", self.run_config.max_steps),
                });
                return Err(AgentError::MaxStepsExceeded(self.run_config.max_steps));
            }

            let response = if let Some(response) = pending_response.take() {
                response
            } else {
                steps += 1;
                if self.run_config.emit_progress {
                    events.push(AgentEvent::StepStarted { step: steps });
                }
                let request = ChatRequest {
                    messages: {
                        let mut messages = skill_activations
                            .iter()
                            .filter_map(|name| self.skills.get(name))
                            .map(|skill| ChatMessage {
                                role: "system".to_string(),
                                content: skill.instructions.clone(),
                            })
                            .collect::<Vec<_>>();
                        messages.extend(
                            session
                                .get_history(self.max_history)
                                .into_iter()
                                .filter(|item| !self.should_drop_message(&item.content))
                                .map(|item| ChatMessage {
                                    role: item.role,
                                    content: item.content,
                                }),
                        );
                        messages
                    },
                    tools: self.tools.names(),
                    model: Some(self.model.clone()),
                };
                let response = provider.chat(request)?;
                events.push(AgentEvent::ProviderResponded {
                    finish_reason: response.finish_reason.clone(),
                    tool_calls: response.tool_calls.len(),
                });
                if self.interrupt_after_step == Some(steps) {
                    self.pending_run = Some(PendingRunState {
                        steps_completed: steps,
                        pending_response: response,
                        skill_activations: skill_activations.clone(),
                    });
                    events.push(AgentEvent::RunInterrupted { step: steps });
                    return Ok(AgentRunReport {
                        status: AgentRunStatus::Interrupted,
                        steps,
                        response: None,
                        events,
                        tool_calls,
                        subagent_calls,
                        skill_activations,
                    });
                }
                response
            };

            if !response.tool_calls.is_empty() {
                for tool_call in response.tool_calls {
                    tool_calls += 1;
                    if self.run_config.emit_tool_hints {
                        events.push(AgentEvent::ToolHint {
                            name: tool_call.name.clone(),
                        });
                    }

                    if tool_call.name == "spawn" {
                        let agent_name = tool_call
                            .arguments
                            .get("agent")
                            .or_else(|| tool_call.arguments.get("name"))
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(|| {
                                AgentError::Tool("spawn missing agent name".to_string())
                            })?;
                        let task = tool_call
                            .arguments
                            .get("task")
                            .or_else(|| tool_call.arguments.get("prompt"))
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(|| AgentError::Tool("spawn missing task".to_string()))?;
                        subagent_calls += 1;
                        events.push(AgentEvent::SubagentStarted {
                            name: agent_name.to_string(),
                            task: task.to_string(),
                        });
                        let result = self
                            .subagents
                            .run(agent_name, task)
                            .map_err(AgentError::Tool)?;
                        session.add_message("tool", format!("{agent_name} => {result}"));
                        events.push(AgentEvent::SubagentFinished {
                            name: agent_name.to_string(),
                            result,
                        });
                        continue;
                    }

                    events.push(AgentEvent::ToolCallStarted {
                        name: tool_call.name.clone(),
                    });
                    let result = self
                        .tools
                        .execute(&tool_call.name, tool_call.arguments)
                        .map_err(|error| AgentError::Tool(error.to_string()))?;
                    if !self.should_drop_message(&result) {
                        session.add_message("tool", format!("{} => {}", tool_call.name, result));
                    }
                    events.push(AgentEvent::ToolCallFinished {
                        name: tool_call.name,
                        result,
                    });
                }
                if !self.run_config.continue_after_tool_calls {
                    events.push(AgentEvent::RunCompleted { steps });
                    return Ok(AgentRunReport {
                        status: AgentRunStatus::Completed,
                        steps,
                        response: None,
                        events,
                        tool_calls,
                        subagent_calls,
                        skill_activations,
                    });
                }
                continue;
            }

            if let Some(content) = response.content {
                if self.should_drop_message(&content) {
                    events.push(AgentEvent::RunCompleted { steps });
                    return Ok(AgentRunReport {
                        status: AgentRunStatus::Completed,
                        steps,
                        response: None,
                        events,
                        tool_calls,
                        subagent_calls,
                        skill_activations,
                    });
                }
                session.add_message("assistant", content.clone());
                events.push(AgentEvent::AssistantMessage {
                    content: content.clone(),
                });
                events.push(AgentEvent::RunCompleted { steps });
                return Ok(AgentRunReport {
                    status: AgentRunStatus::Completed,
                    steps,
                    response: Some(content),
                    events,
                    tool_calls,
                    subagent_calls,
                    skill_activations,
                });
            }

            events.push(AgentEvent::RunCompleted { steps });
            return Ok(AgentRunReport {
                status: AgentRunStatus::Completed,
                steps,
                response: None,
                events,
                tool_calls,
                subagent_calls,
                skill_activations,
            });
        }
    }

    pub fn run_once(
        &mut self,
        provider: &dyn LlmProvider,
        session: &mut Session,
        user_input: &str,
    ) -> Result<Option<String>, AgentError> {
        Ok(self.run_turn(provider, session, user_input)?.response)
    }

    fn persist_user_message(&self, session: &mut Session, input: &str) {
        if self.run_config.merge_consecutive_user {
            if let Some(last) = session.messages.last_mut() {
                if last.role == "user" && !self.should_drop_message(input) {
                    if last.content.is_empty() {
                        last.content = input.to_string();
                    } else {
                        last.content = format!("{}\n{}", last.content, input);
                    }
                    return;
                }
            }
        }
        if !self.should_drop_message(input) {
            session.add_message("user", input);
        }
    }

    fn should_drop_message(&self, content: &str) -> bool {
        self.run_config.drop_empty_messages && content.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nanobot_provider::{ChatRequest, LlmProvider, LlmResponse, ProviderError, StaticProvider};
    use nanobot_session::Session;

    use crate::{AgentEvent, AgentLoop, AgentRunConfig, AgentRunStatus};

    #[test]
    fn appends_user_and_assistant_messages_for_single_turn() {
        let provider = StaticProvider::new("offline/test", "assistant");
        let mut session = Session::new("cli:local");
        let mut loop_ = AgentLoop::new("offline/test");
        loop_.register_tool("message", "send outbound message");

        let response = loop_
            .run_once(&provider, &mut session, "hello")
            .expect("agent loop should succeed");

        assert_eq!(response.as_deref(), Some("assistant: hello"));
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[1].role, "assistant");
    }

    #[derive(Clone, Default)]
    struct RecordingProvider {
        requests: Arc<Mutex<Vec<ChatRequest>>>,
    }

    impl LlmProvider for RecordingProvider {
        fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
            self.requests.lock().expect("lock").push(request);
            Ok(LlmResponse {
                content: Some("ok".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            })
        }

        fn default_model(&self) -> &str {
            "offline/recording"
        }
    }

    #[test]
    fn default_loop_registers_builtin_tools_into_provider_request() {
        let provider = RecordingProvider::default();
        let mut session = Session::new("cli:tools");
        let mut loop_ = AgentLoop::new("offline/tools");

        let _ = loop_
            .run_once(&provider, &mut session, "hello")
            .expect("agent loop should succeed");

        let requests = provider.requests.lock().expect("lock");
        let tools = &requests[0].tools;
        assert!(tools.contains(&"shell".to_string()));
        assert!(tools.contains(&"filesystem".to_string()));
        assert!(tools.contains(&"web".to_string()));
        assert!(tools.contains(&"message".to_string()));
    }

    #[derive(Clone, Default)]
    struct ToolCallingProvider {
        call_count: Arc<Mutex<usize>>,
    }

    impl LlmProvider for ToolCallingProvider {
        fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
            let mut count = self.call_count.lock().expect("lock");
            *count += 1;
            if *count == 1 {
                return Ok(LlmResponse {
                    content: None,
                    tool_calls: vec![nanobot_provider::ToolCallRequest {
                        id: "tool-1".to_string(),
                        name: "shell".to_string(),
                        arguments: serde_json::json!({"command": "echo loop-tool"}),
                    }],
                    finish_reason: "tool_calls".to_string(),
                });
            }

            let saw_tool_result = request
                .messages
                .iter()
                .any(|message| message.content.contains("loop-tool"));
            Ok(LlmResponse {
                content: Some(if saw_tool_result {
                    "final: tool used".to_string()
                } else {
                    "final: missing tool".to_string()
                }),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            })
        }

        fn default_model(&self) -> &str {
            "offline/tool-calling"
        }
    }

    #[test]
    fn agent_loop_executes_tool_calls_before_final_response() {
        let provider = ToolCallingProvider::default();
        let mut session = Session::new("cli:tool-loop");
        let mut loop_ = AgentLoop::new("offline/tool-calling");

        let response = loop_
            .run_once(&provider, &mut session, "run a tool")
            .expect("agent loop should succeed");

        assert_eq!(response.as_deref(), Some("final: tool used"));
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.content.contains("loop-tool")),
            "stub should fail until tool call results are stored"
        );
    }

    #[test]
    fn runtime_v2_exposes_default_config() {
        let config = AgentRunConfig::default();

        assert_eq!(config.max_steps, 5);
        assert!(config.merge_consecutive_user);
        assert!(config.drop_empty_messages);
        assert!(config.emit_progress);
        assert!(config.emit_tool_hints);
    }

    #[test]
    fn runtime_v2_collects_events_and_completion_report() {
        let provider = StaticProvider::new("offline/test", "assistant");
        let mut session = Session::new("cli:report");
        let mut loop_ = AgentLoop::new("offline/test");

        let report = loop_
            .run_turn(&provider, &mut session, "hello runtime")
            .expect("runtime should succeed");

        assert_eq!(report.status, AgentRunStatus::Completed);
        assert_eq!(report.response.as_deref(), Some("assistant: hello runtime"));
        assert_eq!(report.steps, 1);
        assert!(
            report
                .events
                .iter()
                .any(|event| matches!(event, AgentEvent::RunStarted { .. }))
        );
        assert!(
            report
                .events
                .iter()
                .any(|event| matches!(event, AgentEvent::AssistantMessage { content } if content == "assistant: hello runtime"))
        );
    }

    #[test]
    fn runtime_v2_enforces_max_steps() {
        let provider = ToolCallingProvider::default();
        let mut session = Session::new("cli:max-steps");
        let mut loop_ = AgentLoop::new("offline/tool-calling");
        loop_.set_run_config(AgentRunConfig {
            max_steps: 1,
            ..AgentRunConfig::default()
        });

        let error = loop_
            .run_turn(&provider, &mut session, "run until limit")
            .expect_err("runtime should stop at max steps");

        assert!(
            error.to_string().contains("max steps"),
            "expected max steps error, got: {error}"
        );
    }

    #[test]
    fn runtime_v2_can_be_cancelled_before_start() {
        let provider = StaticProvider::new("offline/test", "assistant");
        let mut session = Session::new("cli:cancelled");
        let mut loop_ = AgentLoop::new("offline/test");
        loop_.request_cancel();

        let report = loop_
            .run_turn(&provider, &mut session, "do not run")
            .expect("cancelled run should not error");

        assert_eq!(report.status, AgentRunStatus::Cancelled);
        assert!(report.response.is_none());
        assert!(
            report
                .events
                .iter()
                .any(|event| matches!(event, AgentEvent::RunCancelled { .. }))
        );
    }

    #[test]
    fn runtime_v2_merges_consecutive_user_messages() {
        let provider = StaticProvider::new("offline/test", "assistant");
        let mut session = Session::new("cli:merge");
        session.add_message("user", "first line");
        let mut loop_ = AgentLoop::new("offline/test");

        let report = loop_
            .run_turn(&provider, &mut session, "second line")
            .expect("runtime should succeed");

        assert_eq!(report.status, AgentRunStatus::Completed);
        assert_eq!(session.messages[0].content, "first line\nsecond line");
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn runtime_v2_drops_empty_user_messages_before_provider_call() {
        let provider = RecordingProvider::default();
        let mut session = Session::new("cli:empty");
        let mut loop_ = AgentLoop::new("offline/test");

        let report = loop_
            .run_turn(&provider, &mut session, "   ")
            .expect("empty run should succeed");

        assert_eq!(report.status, AgentRunStatus::Completed);
        assert!(report.response.is_none());
        assert!(provider.requests.lock().expect("lock").is_empty());
        assert!(session.messages.is_empty());
    }

    #[test]
    fn runtime_v2_emits_tool_hint_and_tool_result_events() {
        let provider = ToolCallingProvider::default();
        let mut session = Session::new("cli:tool-events");
        let mut loop_ = AgentLoop::new("offline/tool-calling");

        let report = loop_
            .run_turn(&provider, &mut session, "run a tool")
            .expect("runtime should succeed");

        assert!(
            report
                .events
                .iter()
                .any(|event| matches!(event, AgentEvent::ToolHint { name } if name == "shell"))
        );
        assert!(
            report.events.iter().any(
                |event| matches!(event, AgentEvent::ToolCallFinished { name, result } if name == "shell" && result.contains("loop-tool"))
            )
        );
    }

    #[test]
    fn run_once_remains_compatible_with_runtime_v2() {
        let provider = StaticProvider::new("offline/test", "assistant");
        let mut session = Session::new("cli:compat");
        let mut loop_ = AgentLoop::new("offline/test");

        let response = loop_
            .run_once(&provider, &mut session, "compat path")
            .expect("run_once should remain compatible");

        assert_eq!(response.as_deref(), Some("assistant: compat path"));
    }

    #[derive(Clone, Default)]
    struct SubagentCallingProvider {
        call_count: Arc<Mutex<usize>>,
    }

    impl LlmProvider for SubagentCallingProvider {
        fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
            let mut count = self.call_count.lock().expect("lock");
            *count += 1;
            if *count == 1 {
                return Ok(LlmResponse {
                    content: None,
                    tool_calls: vec![nanobot_provider::ToolCallRequest {
                        id: "spawn-1".to_string(),
                        name: "spawn".to_string(),
                        arguments: serde_json::json!({
                            "agent": "researcher",
                            "task": "summarize issue"
                        }),
                    }],
                    finish_reason: "tool_calls".to_string(),
                });
            }

            let saw_subagent_result = request
                .messages
                .iter()
                .any(|message| message.content.contains("researcher =>"));
            Ok(LlmResponse {
                content: Some(if saw_subagent_result {
                    "subagent integrated".to_string()
                } else {
                    "missing subagent".to_string()
                }),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            })
        }

        fn default_model(&self) -> &str {
            "offline/subagent-calling"
        }
    }

    #[test]
    fn runtime_v2_executes_registered_subagent_via_spawn_tool() {
        let provider = SubagentCallingProvider::default();
        let mut session = Session::new("cli:subagent");
        let mut loop_ = AgentLoop::new("offline/subagent");
        loop_.register_subagent_static("researcher", "research");

        let report = loop_
            .run_turn(&provider, &mut session, "delegate this")
            .expect("runtime should succeed");

        assert_eq!(report.status, AgentRunStatus::Completed);
        assert_eq!(report.response.as_deref(), Some("subagent integrated"));
        assert_eq!(report.subagent_calls, 1);
        assert!(session.messages.iter().any(|message| {
            message
                .content
                .contains("researcher => research: summarize issue")
        }));
        assert!(report.events.iter().any(|event| matches!(
            event,
            AgentEvent::SubagentStarted { name, task }
                if name == "researcher" && task == "summarize issue"
        )));
        assert!(report.events.iter().any(|event| matches!(
            event,
            AgentEvent::SubagentFinished { name, result }
                if name == "researcher" && result == "research: summarize issue"
        )));
    }

    #[test]
    fn runtime_v2_fails_when_spawn_references_unknown_subagent() {
        let provider = SubagentCallingProvider::default();
        let mut session = Session::new("cli:subagent-missing");
        let mut loop_ = AgentLoop::new("offline/subagent");

        let error = loop_
            .run_turn(&provider, &mut session, "delegate this")
            .expect_err("runtime should fail");

        assert!(
            error.to_string().contains("unknown subagent"),
            "expected unknown subagent error, got: {error}"
        );
    }

    #[test]
    fn runtime_v2_loads_skills_from_directory_and_activates_them() {
        let dir = std::env::temp_dir().join(format!("nanobot-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("skills dir");
        std::fs::write(
            dir.join("planner.md"),
            "Plan carefully before writing code.",
        )
        .expect("skill file");

        let provider = RecordingProvider::default();
        let mut session = Session::new("cli:skills");
        let mut loop_ = AgentLoop::new("offline/skills");
        loop_
            .load_skills_from_dir(&dir)
            .expect("skills should load");

        let report = loop_
            .run_turn(&provider, &mut session, "@planner design this feature")
            .expect("runtime should succeed");

        assert_eq!(report.skill_activations, vec!["planner".to_string()]);
        assert!(report.events.iter().any(
            |event| matches!(event, AgentEvent::SkillActivated { name } if name == "planner")
        ));
        let requests = provider.requests.lock().expect("lock");
        assert!(
            requests[0]
                .messages
                .iter()
                .any(|message| message.role == "system"
                    && message.content.contains("Plan carefully"))
        );
    }

    #[test]
    fn runtime_v2_ignores_unknown_skill_mentions() {
        let provider = RecordingProvider::default();
        let mut session = Session::new("cli:skills-unknown");
        let mut loop_ = AgentLoop::new("offline/skills");

        let report = loop_
            .run_turn(&provider, &mut session, "@missing continue normally")
            .expect("runtime should succeed");

        assert!(report.skill_activations.is_empty());
        let requests = provider.requests.lock().expect("lock");
        assert!(
            requests[0]
                .messages
                .iter()
                .all(|message| !(message.role == "system" && message.content.contains("missing")))
        );
    }

    #[test]
    fn runtime_v2_skips_empty_skill_files_when_loading_directory() {
        let dir = std::env::temp_dir().join(format!("nanobot-empty-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("skills dir");
        std::fs::write(dir.join("empty.md"), "   ").expect("empty skill file");

        let mut loop_ = AgentLoop::new("offline/skills");
        let loaded = loop_
            .load_skills_from_dir(&dir)
            .expect("skills should load");

        assert_eq!(loaded, 0);
    }

    #[test]
    fn runtime_v2_interrupts_before_tool_execution_and_can_resume() {
        let provider = ToolCallingProvider::default();
        let mut session = Session::new("cli:interrupt");
        let mut loop_ = AgentLoop::new("offline/interrupt");
        loop_.request_interrupt_after_step(1);

        let first = loop_
            .run_turn(&provider, &mut session, "run a tool")
            .expect("interrupted run should succeed");

        assert_eq!(first.status, AgentRunStatus::Interrupted);
        assert!(first.response.is_none());
        assert!(
            first
                .events
                .iter()
                .any(|event| matches!(event, AgentEvent::RunInterrupted { step } if *step == 1))
        );
        assert!(
            session
                .messages
                .iter()
                .all(|message| !message.content.contains("loop-tool")),
            "tool should not have run before interruption"
        );

        loop_.clear_interrupt();
        let resumed = loop_
            .resume_turn(&provider, &mut session)
            .expect("resume should succeed");

        assert_eq!(resumed.status, AgentRunStatus::Completed);
        assert_eq!(resumed.response.as_deref(), Some("final: tool used"));
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.content.contains("loop-tool"))
        );
    }

    #[test]
    fn runtime_v2_resume_without_pending_state_is_a_noop() {
        let provider = StaticProvider::new("offline/test", "assistant");
        let mut session = Session::new("cli:no-resume");
        let mut loop_ = AgentLoop::new("offline/test");

        let report = loop_
            .resume_turn(&provider, &mut session)
            .expect("resume should not fail");

        assert_eq!(report.status, AgentRunStatus::Ready);
        assert!(report.events.is_empty());
    }

    #[test]
    fn runtime_v2_can_disable_tool_loop_and_return_after_first_provider_response() {
        let provider = ToolCallingProvider::default();
        let mut session = Session::new("cli:single-step");
        let mut loop_ = AgentLoop::new("offline/tool-calling");
        loop_.set_run_config(AgentRunConfig {
            continue_after_tool_calls: false,
            ..AgentRunConfig::default()
        });

        let report = loop_
            .run_turn(&provider, &mut session, "run a tool")
            .expect("run should succeed");

        assert_eq!(report.status, AgentRunStatus::Completed);
        assert!(report.response.is_none());
        assert_eq!(report.tool_calls, 1);
        assert!(report.events.iter().any(|event| matches!(
            event,
            AgentEvent::RunCompleted { steps } if *steps == 1
        )));
    }

    #[test]
    fn runtime_v2_can_resume_across_multiple_interruptions() {
        let provider = ToolCallingProvider::default();
        let mut session = Session::new("cli:multi-interrupt");
        let mut loop_ = AgentLoop::new("offline/tool-calling");
        loop_.request_interrupt_after_step(1);

        let first = loop_
            .run_turn(&provider, &mut session, "run a tool")
            .expect("first run should interrupt");
        assert_eq!(first.status, AgentRunStatus::Interrupted);

        loop_.clear_interrupt();
        loop_.request_interrupt_after_step(2);
        let second = loop_
            .resume_turn(&provider, &mut session)
            .expect("second run should interrupt");
        assert_eq!(second.status, AgentRunStatus::Interrupted);

        loop_.clear_interrupt();
        let final_report = loop_
            .resume_turn(&provider, &mut session)
            .expect("final resume should complete");
        assert_eq!(final_report.status, AgentRunStatus::Completed);
        assert_eq!(final_report.response.as_deref(), Some("final: tool used"));
    }
}

use std::path::Path;

use nanobot_provider::{ChatMessage, ChatRequest, LlmProvider, LlmResponse};
use nanobot_session::Session;

use crate::{
    AgentError, AgentEvent, AgentLoop, AgentRunConfig, AgentRunReport, AgentRunStatus,
    PendingRunState, SkillRegistry, SubagentManager,
};

impl AgentLoop {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_history: 200,
            tools: nanobot_tools::ToolRegistry::with_builtin_defaults(),
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

    pub fn run_once(
        &mut self,
        provider: &dyn LlmProvider,
        session: &mut Session,
        user_input: &str,
    ) -> Result<Option<String>, AgentError> {
        Ok(self.run_turn(provider, session, user_input)?.response)
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

use nanobot_provider::LlmResponse;
use nanobot_tools::ToolRegistry;

use crate::{SkillRegistry, SubagentManager};

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
pub(crate) struct PendingRunState {
    pub(crate) steps_completed: usize,
    pub(crate) pending_response: LlmResponse,
    pub(crate) skill_activations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AgentLoop {
    pub(crate) model: String,
    pub(crate) max_history: usize,
    pub(crate) compact_keep_recent: usize,
    pub(crate) tools: ToolRegistry,
    pub(crate) run_config: AgentRunConfig,
    pub(crate) cancel_requested: bool,
    pub(crate) interrupt_after_step: Option<usize>,
    pub(crate) pending_run: Option<PendingRunState>,
    pub(crate) subagents: SubagentManager,
    pub(crate) skills: SkillRegistry,
}

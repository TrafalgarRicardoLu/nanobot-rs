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

#[derive(Clone, Default)]
struct CompactingProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl LlmProvider for CompactingProvider {
    fn chat(&self, request: ChatRequest) -> Result<LlmResponse, ProviderError> {
        self.requests.lock().expect("lock").push(request.clone());
        let is_compact_request = request.tools.is_empty()
            && request.messages.iter().any(|message| {
                message.role == "system"
                    && message
                        .content
                        .clone()
                        .unwrap_or_default()
                        .contains("compact the conversation history")
            });
        if is_compact_request {
            return Ok(LlmResponse {
                content: Some("summary of earlier context".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            });
        }

        Ok(LlmResponse {
            content: Some("assistant after compact".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        })
    }

    fn default_model(&self) -> &str {
        "offline/compact"
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

        let saw_tool_result = request.messages.iter().any(|message| {
            message
                .content
                .clone()
                .unwrap_or_default()
                .contains("loop-tool")
        });
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
        session.messages.iter().any(|message| {
            message
                .content
                .clone()
                .unwrap_or_default()
                .contains("loop-tool")
        }),
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
    assert_eq!(
        session.messages[0].content.as_deref(),
        Some("first line\nsecond line")
    );
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

#[test]
fn runtime_v2_compacts_old_messages_before_main_provider_request() {
    let provider = CompactingProvider::default();
    let mut session = Session::new("cli:compact");
    for turn in 0..5 {
        session.add_message("user", format!("user message {turn}"));
        session.add_message("assistant", format!("assistant message {turn}"));
    }
    let mut loop_ = AgentLoop::new("offline/compact");
    loop_.max_history = 6;
    loop_.compact_keep_recent = 4;

    let report = loop_
        .run_turn(&provider, &mut session, "latest question")
        .expect("runtime should compact and succeed");

    assert_eq!(report.status, AgentRunStatus::Completed);
    assert_eq!(report.response.as_deref(), Some("assistant after compact"));
    assert!(session.messages.iter().any(|message| {
        message.role == "system"
            && message.content.as_deref() == Some("summary of earlier context")
            && message.metadata.get("kind").map(String::as_str) == Some("compact_summary")
    }));
    assert!(
        session
            .messages
            .iter()
            .all(|message| { message.content.as_deref().unwrap_or_default() != "user message 0" })
    );

    let requests = provider.requests.lock().expect("lock");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].tools.is_empty());
    assert!(requests[0].messages.iter().any(|message| {
        message
            .content
            .clone()
            .unwrap_or_default()
            .contains("compact the conversation history")
    }));
    assert!(requests[1].messages.iter().any(|message| {
        message.role == "system" && message.content.as_deref() == Some("summary of earlier context")
    }));
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| { message.content.as_deref() != Some("user message 0") })
    );
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

        let saw_subagent_result = request.messages.iter().any(|message| {
            message
                .content
                .clone()
                .unwrap_or_default()
                .contains("research: summarize issue")
        });
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
            .clone()
            .unwrap_or_default()
            .contains("research: summarize issue")
    }));
    assert!(session.messages.iter().any(
        |message| message.role == "tool" && message.tool_call_id.as_deref() == Some("spawn-1")
    ));
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
    assert!(
        report
            .events
            .iter()
            .any(|event| matches!(event, AgentEvent::SkillActivated { name } if name == "planner"))
    );
    let requests = provider.requests.lock().expect("lock");
    assert!(requests[0].messages.iter().any(|message| {
        message.role == "system"
            && message
                .content
                .clone()
                .unwrap_or_default()
                .contains("Plan carefully")
    }));
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
    assert!(requests[0].messages.iter().all(|message| {
        !(message.role == "system"
            && message
                .content
                .clone()
                .unwrap_or_default()
                .contains("missing"))
    }));
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
        session.messages.iter().all(|message| !message
            .content
            .clone()
            .unwrap_or_default()
            .contains("loop-tool")),
        "tool should not have run before interruption"
    );

    loop_.clear_interrupt();
    let resumed = loop_
        .resume_turn(&provider, &mut session)
        .expect("resume should succeed");

    assert_eq!(resumed.status, AgentRunStatus::Completed);
    assert_eq!(resumed.response.as_deref(), Some("final: tool used"));
    assert!(session.messages.iter().any(|message| {
        message
            .content
            .clone()
            .unwrap_or_default()
            .contains("loop-tool")
    }));
}

#[test]
fn runtime_v2_stores_assistant_tool_calls_and_tool_call_ids() {
    let provider = ToolCallingProvider::default();
    let mut session = Session::new("cli:tool-schema");
    let mut loop_ = AgentLoop::new("offline/tool-calling");

    let report = loop_
        .run_turn(&provider, &mut session, "run a tool")
        .expect("runtime should succeed");

    assert_eq!(report.status, AgentRunStatus::Completed);
    assert!(session.messages.iter().any(|message| {
        message.role == "assistant"
            && message.tool_calls
                == vec![nanobot_session::StoredToolCall {
                    id: "tool-1".to_string(),
                    name: "shell".to_string(),
                    arguments: serde_json::json!({"command": "echo loop-tool"}),
                }]
    }));
    assert!(session.messages.iter().any(|message| {
        message.role == "tool"
            && message.tool_call_id.as_deref() == Some("tool-1")
            && message
                .content
                .clone()
                .unwrap_or_default()
                .contains("loop-tool")
    }));
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

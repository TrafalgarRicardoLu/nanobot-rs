use std::collections::HashMap;
use std::path::PathBuf;

use nanobot_bus::OutboundMessage;
use serde_json::Value;

use crate::handlers::{execute_cron, execute_filesystem, execute_message, execute_shell};
use crate::{ToolDefinition, ToolError};

#[derive(Debug, Clone)]
pub struct ToolRegistry {
    pub(crate) definitions: Vec<ToolDefinition>,
    pub(crate) cron_jobs: HashMap<String, u64>,
    pub(crate) workspace_root: PathBuf,
    pub(crate) message_target: Option<(String, String)>,
    pub(crate) outbound_messages: Vec<OutboundMessage>,
}

impl ToolRegistry {
    pub fn register(&mut self, definition: ToolDefinition) {
        self.definitions.push(definition);
    }

    pub fn set_workspace_root(&mut self, root: impl Into<PathBuf>) {
        self.workspace_root = root.into();
    }

    pub fn set_message_target(&mut self, channel: impl Into<String>, chat_id: impl Into<String>) {
        self.message_target = Some((channel.into(), chat_id.into()));
    }

    pub fn take_outbound_messages(&mut self) -> Vec<OutboundMessage> {
        std::mem::take(&mut self.outbound_messages)
    }

    pub fn names(&self) -> Vec<String> {
        self.definitions
            .iter()
            .map(|item| item.name.clone())
            .collect()
    }

    pub fn with_builtin_defaults() -> Self {
        let mut registry = Self::default();
        for (name, description) in [
            ("shell", "run shell commands"),
            ("filesystem", "read and write workspace files"),
            ("web", "fetch web pages and search"),
            ("message", "send outbound chat messages"),
            ("spawn", "spawn delegated subagents"),
            ("cron", "manage scheduled jobs"),
            ("mcp", "use model context protocol tools"),
        ] {
            registry.register(ToolDefinition::new(name, description));
        }
        registry
    }

    pub fn execute(&mut self, name: &str, arguments: Value) -> Result<String, ToolError> {
        match name {
            "shell" => execute_shell(arguments),
            "filesystem" => execute_filesystem(self, arguments),
            "web" => Ok("web tool placeholder".to_string()),
            "message" => execute_message(self, arguments),
            "spawn" => Ok("spawn tool placeholder".to_string()),
            "cron" => execute_cron(self, arguments),
            "mcp" => Ok("mcp tool placeholder".to_string()),
            other => Err(ToolError::UnknownTool(other.to_string())),
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            definitions: Vec::new(),
            cron_jobs: HashMap::new(),
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            message_target: None,
            outbound_messages: Vec::new(),
        }
    }
}

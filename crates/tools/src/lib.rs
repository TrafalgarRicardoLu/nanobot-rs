use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nanobot_bus::OutboundMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolRegistry {
    definitions: Vec<ToolDefinition>,
    cron_jobs: HashMap<String, u64>,
    workspace_root: PathBuf,
    message_target: Option<(String, String)>,
    outbound_messages: Vec<OutboundMessage>,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("invalid arguments for {tool}: {message}")]
    InvalidArguments { tool: String, message: String },
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("path escapes workspace root: {0}")]
    PathEscapesWorkspace(String),
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
            "shell" => self.execute_shell(arguments),
            "filesystem" => self.execute_filesystem(arguments),
            "web" => Ok("web tool placeholder".to_string()),
            "message" => self.execute_message(arguments),
            "spawn" => Ok("spawn tool placeholder".to_string()),
            "cron" => self.execute_cron(arguments),
            "mcp" => Ok("mcp tool placeholder".to_string()),
            other => Err(ToolError::UnknownTool(other.to_string())),
        }
    }

    fn execute_shell(&self, arguments: Value) -> Result<String, ToolError> {
        let command = arguments
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "shell".to_string(),
                message: "missing command".to_string(),
            })?;
        let output = Command::new("zsh")
            .arg("-lc")
            .arg(command)
            .output()
            .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(if stdout.is_empty() {
                "(ok)".to_string()
            } else {
                stdout
            })
        } else {
            Err(ToolError::CommandFailed(if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            }))
        }
    }

    fn execute_cron(&mut self, arguments: Value) -> Result<String, ToolError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("add");
        match action {
            "add" => {
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        tool: "cron".to_string(),
                        message: "missing name".to_string(),
                    })?;
                let interval = arguments
                    .get("interval")
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                self.cron_jobs.insert(name.to_string(), interval);
                Ok(format!("scheduled cron job {name} every {interval} ticks"))
            }
            "list" => {
                let mut names: Vec<_> = self.cron_jobs.keys().cloned().collect();
                names.sort();
                Ok(names.join(", "))
            }
            _ => Err(ToolError::InvalidArguments {
                tool: "cron".to_string(),
                message: format!("unsupported action: {action}"),
            }),
        }
    }

    fn execute_filesystem(&mut self, arguments: Value) -> Result<String, ToolError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("read");
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "filesystem".to_string(),
                message: "missing path".to_string(),
            })?;
        let resolved = self.resolve_workspace_path(path)?;
        match action {
            "read" => fs::read_to_string(&resolved)
                .map_err(|error| ToolError::CommandFailed(error.to_string())),
            "write" => {
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(parent) = resolved.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                }
                fs::write(&resolved, content)
                    .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                Ok(format!("wrote {}", resolved.display()))
            }
            "append" => {
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(parent) = resolved.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                }
                let mut existing = if resolved.exists() {
                    fs::read_to_string(&resolved)
                        .map_err(|error| ToolError::CommandFailed(error.to_string()))?
                } else {
                    String::new()
                };
                existing.push_str(content);
                fs::write(&resolved, existing)
                    .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                Ok(format!("appended {}", resolved.display()))
            }
            "exists" => Ok(resolved.exists().to_string()),
            "mkdir" => {
                fs::create_dir_all(&resolved)
                    .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                Ok(format!("created {}", resolved.display()))
            }
            "delete" => {
                if resolved.is_dir() {
                    let _ = fs::remove_dir_all(&resolved);
                } else {
                    let _ = fs::remove_file(&resolved);
                }
                Ok(format!("deleted {}", resolved.display()))
            }
            "replace" => {
                let old = arguments
                    .get("old")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        tool: "filesystem".to_string(),
                        message: "missing old".to_string(),
                    })?;
                let new = arguments
                    .get("new")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        tool: "filesystem".to_string(),
                        message: "missing new".to_string(),
                    })?;
                let content = fs::read_to_string(&resolved)
                    .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                let updated = content.replace(old, new);
                fs::write(&resolved, updated)
                    .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                Ok(format!("replaced {}", resolved.display()))
            }
            "list" => {
                let entries = fs::read_dir(&resolved)
                    .map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                let mut names = Vec::new();
                for entry in entries {
                    let entry =
                        entry.map_err(|error| ToolError::CommandFailed(error.to_string()))?;
                    names.push(entry.file_name().to_string_lossy().to_string());
                }
                names.sort();
                Ok(names.join("\n"))
            }
            other => Err(ToolError::InvalidArguments {
                tool: "filesystem".to_string(),
                message: format!("unsupported action: {other}"),
            }),
        }
    }

    fn execute_message(&mut self, arguments: Value) -> Result<String, ToolError> {
        let content = arguments
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "message".to_string(),
                message: "missing content".to_string(),
            })?;
        let (channel, chat_id) =
            self.message_target
                .clone()
                .ok_or_else(|| ToolError::InvalidArguments {
                    tool: "message".to_string(),
                    message: "missing message target".to_string(),
                })?;
        let reply_to = arguments
            .get("reply_to")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        self.outbound_messages.push(OutboundMessage {
            channel,
            chat_id,
            content: content.to_string(),
            reply_to,
            metadata: HashMap::new(),
        });
        Ok(format!("queued message: {content}"))
    }

    fn resolve_workspace_path(&self, path: &str) -> Result<PathBuf, ToolError> {
        let joined = self.workspace_root.join(path);
        let normalized = normalize_path(&joined);
        let normalized_root = normalize_path(&self.workspace_root);
        if !normalized.starts_with(&normalized_root) {
            return Err(ToolError::PathEscapesWorkspace(path.to_string()));
        }
        Ok(normalized)
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

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests;

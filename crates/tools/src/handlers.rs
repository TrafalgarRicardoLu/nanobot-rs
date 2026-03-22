use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nanobot_bus::OutboundMessage;
use serde_json::Value;

use crate::{ToolError, ToolRegistry};

pub(crate) fn execute_shell(arguments: Value) -> Result<String, ToolError> {
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

pub(crate) fn execute_cron(
    registry: &mut ToolRegistry,
    arguments: Value,
) -> Result<String, ToolError> {
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
            registry.cron_jobs.insert(name.to_string(), interval);
            Ok(format!("scheduled cron job {name} every {interval} ticks"))
        }
        "list" => {
            let mut names: Vec<_> = registry.cron_jobs.keys().cloned().collect();
            names.sort();
            Ok(names.join(", "))
        }
        _ => Err(ToolError::InvalidArguments {
            tool: "cron".to_string(),
            message: format!("unsupported action: {action}"),
        }),
    }
}

pub(crate) fn execute_filesystem(
    registry: &ToolRegistry,
    arguments: Value,
) -> Result<String, ToolError> {
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
    let resolved = resolve_workspace_path(&registry.workspace_root, path)?;
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
                let entry = entry.map_err(|error| ToolError::CommandFailed(error.to_string()))?;
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

pub(crate) fn execute_message(
    registry: &mut ToolRegistry,
    arguments: Value,
) -> Result<String, ToolError> {
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments {
            tool: "message".to_string(),
            message: "missing content".to_string(),
        })?;
    let (channel, chat_id) =
        registry
            .message_target
            .clone()
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "message".to_string(),
                message: "missing message target".to_string(),
            })?;
    let reply_to = arguments
        .get("reply_to")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    registry.outbound_messages.push(OutboundMessage {
        channel,
        chat_id,
        content: content.to_string(),
        reply_to,
        metadata: HashMap::new(),
    });
    Ok(format!("queued message: {content}"))
}

fn resolve_workspace_path(root: &Path, path: &str) -> Result<PathBuf, ToolError> {
    let joined = root.join(path);
    let normalized = normalize_path(&joined);
    let normalized_root = normalize_path(root);
    if !normalized.starts_with(&normalized_root) {
        return Err(ToolError::PathEscapesWorkspace(path.to_string()));
    }
    Ok(normalized)
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

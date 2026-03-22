use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::util::epoch_string;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    pub timestamp: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<StoredToolCall>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl StoredMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(content.into()),
            timestamp: epoch_string(),
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub key: String,
    pub messages: Vec<StoredMessage>,
    pub created_at: String,
    pub updated_at: String,
    pub metadata: HashMap<String, String>,
    pub last_consolidated: usize,
}

impl Session {
    pub fn new(key: impl Into<String>) -> Self {
        let now = epoch_string();
        Self {
            key: key.into(),
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            metadata: HashMap::new(),
            last_consolidated: 0,
        }
    }

    pub fn add_message(&mut self, role: impl Into<String>, content: impl Into<String>) {
        self.messages.push(StoredMessage::new(role, content));
        self.updated_at = epoch_string();
    }

    pub fn add_structured_message(&mut self, message: StoredMessage) {
        self.messages.push(message);
        self.updated_at = epoch_string();
    }

    pub fn get_history(&self, max_messages: usize) -> Vec<StoredMessage> {
        let unconsolidated = &self.messages[self.last_consolidated.min(self.messages.len())..];
        let start = unconsolidated.len().saturating_sub(max_messages);
        let mut sliced = unconsolidated[start..].to_vec();

        if let Some(index) = sliced.iter().position(|msg| msg.role == "user") {
            sliced.drain(0..index);
        } else {
            sliced.clear();
        }

        sliced
    }
}

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
        let start = self.messages.len().saturating_sub(max_messages);
        let mut sliced = self.messages[start..].to_vec();
        let first_user = sliced.iter().position(|msg| msg.role == "user");
        let leading_summary_count = sliced
            .iter()
            .take_while(|msg| {
                msg.metadata.get("kind").map(String::as_str) == Some("compact_summary")
            })
            .count();

        match first_user {
            Some(index) if index > leading_summary_count => {
                sliced.drain(leading_summary_count..index);
                sliced
            }
            Some(_) => sliced,
            None if leading_summary_count > 0 => sliced[..leading_summary_count].to_vec(),
            None => Vec::new(),
        }
    }
}

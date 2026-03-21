use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl StoredMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
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

#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions_dir: PathBuf,
    memories_dir: PathBuf,
}

impl SessionManager {
    pub fn new(sessions_dir: impl Into<PathBuf>) -> Result<Self, SessionError> {
        let sessions_dir = sessions_dir.into();
        let base_dir = sessions_dir
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let memories_dir = base_dir.join("memories");
        fs::create_dir_all(&sessions_dir)?;
        fs::create_dir_all(&memories_dir)?;
        Ok(Self {
            sessions_dir,
            memories_dir,
        })
    }

    pub fn save(&self, session: &Session) -> Result<PathBuf, SessionError> {
        let path = self.session_path(&session.key);
        let mut lines = Vec::with_capacity(session.messages.len() + 1);
        lines.push(serde_json::to_string(&MetadataLine::from(session))?);
        for message in &session.messages {
            lines.push(serde_json::to_string(message)?);
        }
        fs::write(&path, lines.join("\n") + "\n")?;
        Ok(path)
    }

    pub fn load(&self, key: &str) -> Result<Option<Session>, SessionError> {
        let path = self.session_path(key);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path)?;
        let mut lines = content.lines();
        let Some(first) = lines.next() else {
            return Ok(None);
        };
        let metadata: MetadataLine = serde_json::from_str(first)?;
        let mut session = Session {
            key: metadata.key,
            messages: Vec::new(),
            created_at: metadata.created_at,
            updated_at: metadata.updated_at,
            metadata: metadata.metadata,
            last_consolidated: metadata.last_consolidated,
        };
        for line in lines {
            if !line.trim().is_empty() {
                session.messages.push(serde_json::from_str(line)?);
            }
        }
        Ok(Some(session))
    }

    pub fn load_or_create(&self, key: &str) -> Result<Session, SessionError> {
        Ok(self.load(key)?.unwrap_or_else(|| Session::new(key)))
    }

    pub fn consolidate(&self, session: &mut Session) -> Result<(), SessionError> {
        let dir = self.memory_dir(&session.key);
        fs::create_dir_all(&dir)?;
        let unconsolidated = &session.messages[self.last_consolidated_index(session)..];
        let turns = unconsolidated
            .iter()
            .filter(|message| message.role == "user")
            .count();
        let tools = unconsolidated
            .iter()
            .flat_map(|message| message.tool_calls.iter().cloned())
            .collect::<Vec<_>>();
        let recent_topics = unconsolidated
            .iter()
            .filter(|message| message.role == "user")
            .map(|message| format!("- {}", message.content))
            .collect::<Vec<_>>();

        let memory = format!(
            "# MEMORY\n\n- session: {}\n- turns: {}\n- tools: {}\n",
            session.key,
            turns,
            if tools.is_empty() {
                "none".to_string()
            } else {
                tools.join(", ")
            }
        );
        let history = format!(
            "# HISTORY\n\n{}\n",
            if recent_topics.is_empty() {
                "- (no recent user messages)".to_string()
            } else {
                recent_topics.join("\n")
            }
        );

        fs::write(dir.join("MEMORY.md"), memory)?;
        fs::write(dir.join("HISTORY.md"), history)?;
        session.last_consolidated = session.messages.len();
        Ok(())
    }

    pub fn maybe_consolidate(
        &self,
        session: &mut Session,
        threshold: usize,
    ) -> Result<bool, SessionError> {
        let unconsolidated_len = session
            .messages
            .len()
            .saturating_sub(self.last_consolidated_index(session));
        if unconsolidated_len >= threshold {
            self.consolidate(session)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn memory_dir(&self, key: &str) -> PathBuf {
        self.memories_dir.join(sanitize_key(key))
    }

    fn session_path(&self, key: &str) -> PathBuf {
        self.sessions_dir
            .join(format!("{}.jsonl", sanitize_key(key)))
    }

    fn last_consolidated_index(&self, session: &Session) -> usize {
        session.last_consolidated.min(session.messages.len())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetadataLine {
    #[serde(rename = "_type")]
    kind: String,
    key: String,
    created_at: String,
    updated_at: String,
    metadata: HashMap<String, String>,
    last_consolidated: usize,
}

impl From<&Session> for MetadataLine {
    fn from(session: &Session) -> Self {
        Self {
            kind: "metadata".to_string(),
            key: session.key.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            metadata: session.metadata.clone(),
            last_consolidated: session.last_consolidated,
        }
    }
}

fn epoch_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn sanitize_key(key: &str) -> String {
    key.replace(':', "_")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    use super::{Session, SessionManager, StoredMessage};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nanobot-rs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir should exist");
        dir
    }

    #[test]
    fn saves_and_loads_jsonl_session_with_metadata_header() {
        let dir = temp_dir("save-load");
        let manager = SessionManager::new(&dir).expect("manager should build");
        let mut session = Session::new("feishu:oc_1");
        session
            .metadata
            .insert("channel".to_string(), "feishu".to_string());
        session.add_message("user", "hello");
        session.add_message("assistant", "world");

        let path = manager.save(&session).expect("session should save");
        let raw = fs::read_to_string(&path).expect("session file should exist");

        assert!(
            raw.lines()
                .next()
                .unwrap_or_default()
                .contains(r#""_type":"metadata""#)
        );

        let loaded = manager
            .load("feishu:oc_1")
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(loaded.key, "feishu:oc_1");
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(
            loaded.metadata.get("channel").map(String::as_str),
            Some("feishu")
        );
    }

    #[test]
    fn get_history_skips_leading_non_user_messages_after_consolidation() {
        let mut session = Session::new("qq:user-1");
        session.add_message("assistant", "orphan assistant");
        session.add_message("tool", "orphan tool");
        session.add_message("user", "hello");
        session.add_message("assistant", "hi");
        session.last_consolidated = 1;

        let history = session.get_history(10);

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].role, "assistant");
    }

    #[test]
    fn saves_and_loads_structured_message_fields() {
        let dir = temp_dir("structured");
        let manager = SessionManager::new(&dir).expect("manager should build");
        let mut session = Session::new("cli:structured");
        session.add_structured_message(StoredMessage {
            role: "assistant".to_string(),
            content: "tool call".to_string(),
            timestamp: "1".to_string(),
            name: Some("assistant".to_string()),
            tool_call_id: Some("call-1".to_string()),
            tool_calls: vec!["filesystem".to_string()],
            metadata: HashMap::from([("kind".to_string(), "tool".to_string())]),
        });

        manager.save(&session).expect("save should work");
        let loaded = manager
            .load("cli:structured")
            .expect("load should work")
            .expect("session should exist");

        assert_eq!(loaded.messages[0].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(
            loaded.messages[0].tool_calls,
            vec!["filesystem".to_string()]
        );
        assert_eq!(
            loaded.messages[0].metadata.get("kind").map(String::as_str),
            Some("tool")
        );
    }

    #[test]
    fn consolidate_writes_memory_and_history_files() {
        let dir = temp_dir("consolidate");
        let manager = SessionManager::new(&dir).expect("manager should build");
        let mut session = Session::new("qq:user-9");
        session.add_message("user", "track my todos");
        session.add_structured_message(StoredMessage {
            role: "assistant".to_string(),
            content: "used filesystem".to_string(),
            timestamp: "2".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: vec!["filesystem".to_string()],
            metadata: HashMap::new(),
        });

        manager
            .consolidate(&mut session)
            .expect("consolidation should work");

        let memory = fs::read_to_string(manager.memory_dir("qq:user-9").join("MEMORY.md"))
            .expect("memory file should exist");
        let history = fs::read_to_string(manager.memory_dir("qq:user-9").join("HISTORY.md"))
            .expect("history file should exist");
        assert!(memory.contains("filesystem"));
        assert!(history.contains("track my todos"));
        assert_eq!(session.last_consolidated, session.messages.len());
    }
}

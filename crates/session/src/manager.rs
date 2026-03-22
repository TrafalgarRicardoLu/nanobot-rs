use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::util::sanitize_key;
use crate::{Session, SessionError};

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
            .flat_map(|message| message.tool_calls.iter().map(|call| call.name.clone()))
            .collect::<Vec<_>>();
        let recent_topics = unconsolidated
            .iter()
            .filter(|message| message.role == "user")
            .map(|message| format!("- {}", message.content.clone().unwrap_or_default()))
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

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::util::sanitize_key;
use crate::{Session, SessionError};

#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions_dir: PathBuf,
}

impl SessionManager {
    pub fn new(sessions_dir: impl Into<PathBuf>) -> Result<Self, SessionError> {
        let sessions_dir = sessions_dir.into();
        fs::create_dir_all(&sessions_dir)?;
        Ok(Self { sessions_dir })
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

    fn session_path(&self, key: &str) -> PathBuf {
        self.sessions_dir
            .join(format!("{}.jsonl", sanitize_key(key)))
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
}

impl From<&Session> for MetadataLine {
    fn from(session: &Session) -> Self {
        Self {
            kind: "metadata".to_string(),
            key: session.key.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            metadata: session.metadata.clone(),
        }
    }
}

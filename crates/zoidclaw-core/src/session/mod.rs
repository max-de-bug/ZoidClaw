//! Session management for conversation history.
//!
//! Sessions are stored as JSONL files for easy persistence and reading.
//! Each line in the file is a JSON object representing a message.

use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;

/// A conversation session with message history.
#[derive(Debug, Clone)]
pub struct Session {
    pub key: String,
    pub messages: Vec<SessionMessage>,
    pub created_at: String,
    pub updated_at: String,
}

/// A single message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<crate::provider::types::ToolCallMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Session {
    pub fn new(key: &str) -> Self {
        let now = chrono::Local::now().to_rfc3339();
        Self {
            key: key.to_string(),
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Add a message to the session.
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push(SessionMessage {
            role: role.to_string(),
            content: Some(content.to_string()),
            timestamp: chrono::Local::now().to_rfc3339(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
        self.updated_at = chrono::Local::now().to_rfc3339();
    }

    /// Add a full chat message to the session.
    pub fn add_chat_message(&mut self, msg: &crate::provider::types::ChatMessage) {
        self.messages.push(SessionMessage {
            role: msg.role.clone(),
            content: msg.content_as_str().map(|s| s.to_string()),
            timestamp: chrono::Local::now().to_rfc3339(),
            tool_calls: msg.tool_calls.clone(),
            tool_call_id: msg.tool_call_id.clone(),
            name: msg.name.clone(),
        });
        self.updated_at = chrono::Local::now().to_rfc3339();
    }

    /// Get message history for LLM context (most recent N messages).
    pub fn get_history(&self, max_messages: usize) -> Vec<crate::provider::types::ChatMessage> {
        let start = if self.messages.len() > max_messages {
            self.messages.len() - max_messages
        } else {
            0
        };

        self.messages[start..]
            .iter()
            .map(|m| crate::provider::types::ChatMessage {
                role: m.role.clone(),
                content: m
                    .content
                    .as_ref()
                    .map(|s| serde_json::Value::String(s.clone())),
                tool_calls: m.tool_calls.clone(),
                tool_call_id: m.tool_call_id.clone(),
                name: m.name.clone(),
            })
            .collect()
    }

    /// Get message history trimmed to fit within an estimated token budget.
    ///
    /// Uses the heuristic `chars / 4 ≈ tokens`. Walks from the *tail* of
    /// the history and includes messages until the budget would be exceeded.
    /// This prevents silent context-window overflow on long conversations.
    ///
    /// At minimum one message is always returned (the most recent) so the
    /// agent always has something to reason about.
    pub fn get_history_within_budget(
        &self,
        max_tokens: usize,
    ) -> Vec<crate::provider::types::ChatMessage> {
        if self.messages.is_empty() {
            return vec![];
        }

        let mut budget = max_tokens;
        // Walk backwards from the end of history
        let mut start = self.messages.len();
        for msg in self.messages.iter().rev() {
            let char_count = msg.content.as_deref().map(|s| s.len()).unwrap_or(0);
            let estimated_tokens = (char_count / 4).max(1); // at least 1 token per message

            if start < self.messages.len() && estimated_tokens > budget {
                // Budget would exceed — stop here (but we already included one)
                break;
            }

            start = start.saturating_sub(1);

            if estimated_tokens >= budget {
                // This message alone fills the budget; include it and stop.
                break;
            }
            budget = budget.saturating_sub(estimated_tokens);
        }

        self.messages[start..]
            .iter()
            .map(|m| crate::provider::types::ChatMessage {
                role: m.role.clone(),
                content: m
                    .content
                    .as_ref()
                    .map(|s| serde_json::Value::String(s.clone())),
                tool_calls: m.tool_calls.clone(),
                tool_call_id: m.tool_call_id.clone(),
                name: m.name.clone(),
            })
            .collect()
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.updated_at = chrono::Local::now().to_rfc3339();
    }
}

/// Manages conversation sessions with file-based persistence.
pub struct SessionManager {
    sessions_dir: PathBuf,
    cache: HashMap<String, Session>,
}

impl SessionManager {
    pub fn new(_workspace: &Path) -> Self {
        let sessions_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".zoidclaw")
            .join("sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        Self {
            sessions_dir,
            cache: HashMap::new(),
        }
    }

    /// Get an existing session or create a new one.
    pub fn get_or_create(&mut self, key: &str) -> &mut Session {
        if !self.cache.contains_key(key) {
            let session = self.load(key).unwrap_or_else(|| Session::new(key));
            self.cache.insert(key.to_string(), session);
        }
        self.cache.get_mut(key).unwrap()
    }

    /// Save a session to disk.
    pub fn save(&self, key: &str) -> anyhow::Result<()> {
        let session = match self.cache.get(key) {
            Some(s) => s,
            None => return Ok(()),
        };

        let path = self.session_path(key);
        let mut lines = Vec::new();

        // Metadata line
        let metadata = serde_json::json!({
            "_type": "metadata",
            "created_at": session.created_at,
            "updated_at": session.updated_at,
        });
        lines.push(serde_json::to_string(&metadata)?);

        // Message lines
        for msg in &session.messages {
            lines.push(serde_json::to_string(msg)?);
        }

        std::fs::write(path, lines.join("\n") + "\n")?;
        Ok(())
    }

    /// Delete a session.
    pub fn delete(&mut self, key: &str) -> bool {
        self.cache.remove(key);
        let path = self.session_path(key);
        if path.exists() {
            std::fs::remove_file(path).is_ok()
        } else {
            false
        }
    }

    /// List all sessions.
    pub fn list_sessions(&self) -> Vec<(String, String)> {
        let mut sessions = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "jsonl") {
                    let key = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .replace('_', ":");

                    // Read the first line for metadata
                    let updated = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|c| c.lines().next().map(|l| l.to_string()))
                        .and_then(|l| serde_json::from_str::<serde_json::Value>(&l).ok())
                        .and_then(|v| v["updated_at"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();

                    sessions.push((key, updated));
                }
            }
        }

        sessions.sort_by(|a, b| b.1.cmp(&a.1));
        sessions
    }

    // ── Private helpers ─────────────────────────────────────────────

    fn session_path(&self, key: &str) -> PathBuf {
        let safe_name = key.replace([':', '/'], "_");
        self.sessions_dir.join(format!("{}.jsonl", safe_name))
    }

    fn load(&self, key: &str) -> Option<Session> {
        let path = self.session_path(key);
        if !path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&path).ok()?;
        let mut messages = Vec::new();
        let mut created_at = String::new();
        let mut updated_at = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                if value.get("_type").and_then(|v| v.as_str()) == Some("metadata") {
                    created_at = value["created_at"].as_str().unwrap_or_default().to_string();
                    updated_at = value["updated_at"].as_str().unwrap_or_default().to_string();
                } else if let Ok(msg) = serde_json::from_value::<SessionMessage>(value) {
                    messages.push(msg);
                }
            } else {
                warn!(line, "Failed to parse session line");
            }
        }

        Some(Session {
            key: key.to_string(),
            messages,
            created_at,
            updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_add_message() {
        let mut session = Session::new("test:session");
        session.add_message("user", "Hello!");
        session.add_message("assistant", "Hi there!");

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[1].content.as_deref(), Some("Hi there!"));
    }

    #[test]
    fn test_session_get_history() {
        let mut session = Session::new("test:session");
        for i in 0..10 {
            session.add_message("user", &format!("Message {}", i));
        }

        let history = session.get_history(5);
        assert_eq!(history.len(), 5);
        assert_eq!(history[0].content_as_str().unwrap(), "Message 5");
    }
}

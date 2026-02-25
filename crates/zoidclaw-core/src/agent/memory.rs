//! Persistent memory system for the agent.
//!
//! Supports daily notes (`memory/YYYY-MM-DD.md`) and long-term memory (`MEMORY.md`).
//! All storage is plain markdown files — easy to read, edit, and version.

use chrono::Local;
use std::path::{Path, PathBuf};

pub struct MemoryStore {
    memory_dir: PathBuf,
    memory_file: PathBuf,
}

impl MemoryStore {
    pub fn new(workspace: &Path) -> Self {
        let memory_dir = workspace.join("memory");
        let memory_file = memory_dir.join("MEMORY.md");
        Self {
            memory_dir,
            memory_file,
        }
    }

    /// Ensure the memory directory exists.
    fn ensure_dir(&self) {
        let _ = std::fs::create_dir_all(&self.memory_dir);
    }

    /// Get today's date string (YYYY-MM-DD).
    fn today_str(&self) -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    /// Get path to today's memory file.
    pub fn today_file(&self) -> PathBuf {
        self.memory_dir.join(format!("{}.md", self.today_str()))
    }

    /// Read today's memory notes.
    pub fn read_today(&self) -> String {
        let path = self.today_file();
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// Append content to today's memory notes.
    pub fn append_today(&self, content: &str) {
        self.ensure_dir();
        let path = self.today_file();

        let full_content = if path.exists() {
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            format!("{}\n{}", existing, content)
        } else {
            format!("# {}\n\n{}", self.today_str(), content)
        };

        let _ = std::fs::write(path, full_content);
    }

    /// Read long-term memory (MEMORY.md).
    pub fn read_long_term(&self) -> String {
        std::fs::read_to_string(&self.memory_file).unwrap_or_default()
    }

    /// Write to long-term memory (MEMORY.md).
    pub fn write_long_term(&self, content: &str) {
        self.ensure_dir();
        let _ = std::fs::write(&self.memory_file, content);
    }

    /// Get memories from the last N days.
    pub fn recent_memories(&self, days: u32) -> String {
        use chrono::Duration;

        let today = Local::now().date_naive();
        let mut memories = Vec::new();

        for i in 0..days {
            let date = today - Duration::days(i as i64);
            let date_str = date.format("%Y-%m-%d").to_string();
            let path = self.memory_dir.join(format!("{}.md", date_str));

            if let Ok(content) = std::fs::read_to_string(path) {
                memories.push(content);
            }
        }

        memories.join("\n\n---\n\n")
    }

    /// Get formatted memory context for inclusion in the system prompt.
    pub fn context(&self) -> String {
        let mut parts = Vec::new();

        let long_term = self.read_long_term();
        if !long_term.is_empty() {
            parts.push(format!("## Long-term Memory\n{}", long_term));
        }

        let today = self.read_today();
        if !today.is_empty() {
            parts.push(format!("## Today's Notes\n{}", today));
        }

        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_memory_store() {
        let tmp = std::env::temp_dir().join("zoidclaw_test_memory");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let store = MemoryStore::new(&tmp);

        // Test long-term memory
        assert!(store.read_long_term().is_empty());
        store.write_long_term("Remember: user prefers dark mode");
        assert_eq!(store.read_long_term(), "Remember: user prefers dark mode");

        // Test daily notes
        store.append_today("Had a meeting about project X");
        let today = store.read_today();
        assert!(today.contains("Had a meeting about project X"));

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }
}

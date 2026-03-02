//! Filesystem tools: read_file, write_file, edit_file, list_dir.
//!
//! These tools give the agent the ability to interact with the local
//! filesystem. When `restrict_to_workspace` is enabled, all paths are
//! validated to be within the workspace directory.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::Tool;

// ── Helpers ─────────────────────────────────────────────────────────

fn resolve_path(raw: &str, workspace: &Path, restrict: bool) -> Result<PathBuf, String> {
    let path = if raw.starts_with("~/") || raw.starts_with("~\\") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(&raw[2..])
    } else {
        PathBuf::from(raw)
    };

    // Canonicalize the resolved path (or at least normalize it)
    let path = if path.exists() {
        path.canonicalize().unwrap_or(path)
    } else {
        path
    };

    if restrict {
        let ws = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf());
        if !path.starts_with(&ws) {
            return Err(format!(
                "Access denied: path '{}' is outside workspace '{}'",
                path.display(),
                ws.display()
            ));
        }
    }

    Ok(path)
}

fn get_string_arg(args: &HashMap<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_int_arg(args: &HashMap<String, Value>, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

// ── ReadFileTool ────────────────────────────────────────────────────

pub struct ReadFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl ReadFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Supports optional line range."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional 1-indexed start line"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional 1-indexed end line (inclusive)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(raw_path) = get_string_arg(&args, "path") else {
            return "Error: 'path' parameter is required".into();
        };

        let path = match resolve_path(&raw_path, &self.workspace, self.restrict) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return format!("Error reading '{}': {}", path.display(), e),
        };

        let start = get_int_arg(&args, "start_line").map(|n| (n - 1).max(0) as usize);
        let end = get_int_arg(&args, "end_line").map(|n| n as usize);

        match (start, end) {
            (Some(s), Some(e)) => {
                let lines: Vec<&str> = content.lines().collect();
                let end = e.min(lines.len());
                lines[s..end].join("\n")
            }
            (Some(s), None) => {
                let lines: Vec<&str> = content.lines().collect();
                lines[s..].join("\n")
            }
            _ => content,
        }
    }
}

// ── WriteFileTool ───────────────────────────────────────────────────

pub struct WriteFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl WriteFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file and parent directories if they don't exist."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(raw_path) = get_string_arg(&args, "path") else {
            return "Error: 'path' parameter is required".into();
        };
        let Some(content) = get_string_arg(&args, "content") else {
            return "Error: 'content' parameter is required".into();
        };

        let path = match resolve_path(&raw_path, &self.workspace, self.restrict) {
            Ok(p) => p,
            Err(e) => return e,
        };

        // Create parent directories
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("Error creating directories: {}", e);
            }
        }

        match std::fs::write(&path, &content) {
            Ok(_) => format!("Wrote {} bytes to '{}'", content.len(), path.display()),
            Err(e) => format!("Error writing '{}': {}", path.display(), e),
        }
    }
}

// ── EditFileTool ────────────────────────────────────────────────────

pub struct EditFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl EditFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact string match with new content."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text"
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(raw_path) = get_string_arg(&args, "path") else {
            return "Error: 'path' parameter is required".into();
        };
        let Some(old_text) = get_string_arg(&args, "old_text") else {
            return "Error: 'old_text' parameter is required".into();
        };
        let Some(new_text) = get_string_arg(&args, "new_text") else {
            return "Error: 'new_text' parameter is required".into();
        };

        let path = match resolve_path(&raw_path, &self.workspace, self.restrict) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return format!("Error reading '{}': {}", path.display(), e),
        };

        let count = content.matches(&old_text).count();
        if count == 0 {
            return format!("Error: '{}' not found in '{}'", old_text, path.display());
        }

        let new_content = content.replacen(&old_text, &new_text, 1);
        match std::fs::write(&path, &new_content) {
            Ok(_) => format!(
                "Replaced 1 occurrence in '{}' ({} total matches)",
                path.display(),
                count
            ),
            Err(e) => format!("Error writing '{}': {}", path.display(), e),
        }
    }
}

// ── ListDirTool ─────────────────────────────────────────────────────

pub struct ListDirTool {
    workspace: PathBuf,
    restrict: bool,
}

impl ListDirTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List the contents of a directory."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(raw_path) = get_string_arg(&args, "path") else {
            return "Error: 'path' parameter is required".into();
        };

        let path = match resolve_path(&raw_path, &self.workspace, self.restrict) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(e) => return format!("Error listing '{}': {}", path.display(), e),
        };

        let mut items: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata();
            let suffix = match &meta {
                Ok(m) if m.is_dir() => "/",
                _ => "",
            };
            let size = match &meta {
                Ok(m) if m.is_file() => format!("  ({} bytes)", m.len()),
                _ => String::new(),
            };
            items.push(format!("{}{}{}", name, suffix, size));
        }

        items.sort();

        if items.is_empty() {
            format!("'{}' is empty", path.display())
        } else {
            items.join("\n")
        }
    }
}

//! Shell execution tool.
//!
//! Allows the agent to run shell commands with configurable timeout
//! and optional workspace restriction.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

use super::Tool;

pub struct ExecTool {
    workspace: PathBuf,
    restrict: bool,
    timeout_secs: u64,
}

impl ExecTool {
    pub fn new(workspace: PathBuf, restrict: bool, timeout_secs: u64) -> Self {
        Self {
            workspace,
            restrict,
            timeout_secs,
        }
    }
}

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "shell_exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its stdout and stderr output."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory (defaults to workspace)"
                },
                "timeout": {
                    "type": "number",
                    "description": "Optional timeout in seconds (default: 30)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
            return "Error: 'command' parameter is required".into();
        };

        let cwd = args
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.clone());

        // Workspace restriction check
        if self.restrict {
            let ws = self
                .workspace
                .canonicalize()
                .unwrap_or_else(|_| self.workspace.clone());
            let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
            if !cwd_canon.starts_with(&ws) {
                return format!(
                    "Access denied: working directory '{}' is outside workspace",
                    cwd.display()
                );
            }
        }

        let timeout = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.timeout_secs);

        debug!(command, cwd = %cwd.display(), timeout, "Executing shell command");

        // Platform-specific shell
        let (shell, flag) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let result = tokio::time::timeout(
            Duration::from_secs(timeout),
            Command::new(shell)
                .arg(flag)
                .arg(command)
                .current_dir(&cwd)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let mut result = String::new();

                if !stdout.is_empty() {
                    result.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str("[stderr]\n");
                    result.push_str(&stderr);
                }

                if exit_code != 0 {
                    result.push_str(&format!("\n[exit code: {}]", exit_code));
                }

                if result.is_empty() {
                    "(no output)".into()
                } else {
                    // Truncate very long output
                    if result.len() > 50_000 {
                        let truncated = &result[..50_000];
                        format!(
                            "{}\n\n... (truncated, {} total bytes)",
                            truncated,
                            result.len()
                        )
                    } else {
                        result
                    }
                }
            }
            Ok(Err(e)) => format!("Error executing command: {}", e),
            Err(_) => format!("Error: command timed out after {} seconds", timeout),
        }
    }
}

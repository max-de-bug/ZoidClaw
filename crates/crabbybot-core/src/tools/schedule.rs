//! LLM-powered scheduling tools.
//!
//! These tools let the agent schedule recurring tasks via natural language.
//! The LLM decides the cron expression or interval and calls these tools.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::Tool;
use crate::cron::{CronService, Schedule};

// ── ScheduleTaskTool ────────────────────────────────────────────────

pub struct ScheduleTaskTool {
    cron: Arc<Mutex<CronService>>,
    /// Default channel to route responses to (e.g., "telegram").
    default_channel: String,
    /// Default chat_id for jobs created in contexts where chat_id is unknown.
    default_chat_id: String,
}

impl ScheduleTaskTool {
    pub fn new(
        cron: Arc<Mutex<CronService>>,
        default_channel: String,
        default_chat_id: String,
    ) -> Self {
        Self {
            cron,
            default_channel,
            default_chat_id,
        }
    }
}

#[async_trait]
impl Tool for ScheduleTaskTool {
    fn name(&self) -> &str {
        "schedule_task"
    }

    fn description(&self) -> &str {
        "Schedule a recurring task. The task message will be sent to the agent \
         at the specified interval or cron schedule. Use this when the user asks \
         to be reminded, wants periodic updates, or says 'every hour/day/etc'."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable name for the task (e.g., 'Check SOL price')"
                },
                "schedule": {
                    "type": "string",
                    "description": "Cron expression (e.g., '0 9 * * *' for 9am daily) or interval with 's' suffix (e.g., '3600s' for every hour, '60s' for every minute)"
                },
                "message": {
                    "type": "string",
                    "description": "The prompt/message to process when the task fires (e.g., 'What is the current SOL price?')"
                }
            },
            "required": ["name", "schedule", "message"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
            return "Error: 'name' parameter is required".into();
        };
        let Some(schedule_str) = args.get("schedule").and_then(|v| v.as_str()) else {
            return "Error: 'schedule' parameter is required".into();
        };
        let Some(message) = args.get("message").and_then(|v| v.as_str()) else {
            return "Error: 'message' parameter is required".into();
        };

        // Parse schedule: "60s" → Interval, otherwise treat as cron expression
        let schedule = if let Some(secs) = schedule_str.strip_suffix('s') {
            match secs.parse::<u64>() {
                Ok(s) if s > 0 => Schedule::Interval { seconds: s },
                _ => return format!("Error: Invalid interval '{}'. Use e.g., '60s' or '3600s'", schedule_str),
            }
        } else {
            Schedule::Cron {
                expression: schedule_str.to_string(),
            }
        };

        let mut cron = self.cron.lock().await;
        match cron.add_job(
            name,
            schedule,
            message,
            &self.default_channel,
            &self.default_chat_id,
        ) {
            Ok(id) => {
                format!(
                    "✅ Scheduled task '{}' (ID: {})\n\
                     Schedule: {}\n\
                     Message: {}",
                    name, id, schedule_str, message
                )
            }
            Err(e) => format!("Error scheduling task: {}", e),
        }
    }
}

// ── ListSchedulesTool ───────────────────────────────────────────────

pub struct ListSchedulesTool {
    cron: Arc<Mutex<CronService>>,
}

impl ListSchedulesTool {
    pub fn new(cron: Arc<Mutex<CronService>>) -> Self {
        Self { cron }
    }
}

#[async_trait]
impl Tool for ListSchedulesTool {
    fn name(&self) -> &str {
        "list_schedules"
    }

    fn description(&self) -> &str {
        "List all scheduled recurring tasks. Shows name, schedule, message, and status."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: HashMap<String, Value>) -> String {
        let cron = self.cron.lock().await;
        let jobs = cron.list_jobs(true);

        if jobs.is_empty() {
            return "No scheduled tasks found.".into();
        }

        let mut output = format!("📋 {} scheduled task(s):\n\n", jobs.len());
        for job in jobs {
            let schedule_str = match &job.schedule {
                Schedule::Cron { expression } => format!("cron: {}", expression),
                Schedule::Interval { seconds } => format!("every {}s", seconds),
            };
            let status = if job.enabled { "✅ enabled" } else { "⏸️ disabled" };
            let last_run = job
                .last_run
                .as_deref()
                .unwrap_or("never");

            output.push_str(&format!(
                "• **{}** ({})\n  ID: `{}`\n  Schedule: {}\n  Message: {}\n  Last run: {}\n\n",
                job.name, status, job.id, schedule_str, job.message, last_run
            ));
        }

        output
    }
}

// ── CancelScheduleTool ──────────────────────────────────────────────

pub struct CancelScheduleTool {
    cron: Arc<Mutex<CronService>>,
}

impl CancelScheduleTool {
    pub fn new(cron: Arc<Mutex<CronService>>) -> Self {
        Self { cron }
    }
}

#[async_trait]
impl Tool for CancelScheduleTool {
    fn name(&self) -> &str {
        "cancel_schedule"
    }

    fn description(&self) -> &str {
        "Cancel a scheduled task by its ID. Use list_schedules first to find the ID."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "The ID of the job to cancel (e.g., 'job_1a2b3c')"
                }
            },
            "required": ["job_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(job_id) = args.get("job_id").and_then(|v| v.as_str()) else {
            return "Error: 'job_id' parameter is required".into();
        };

        let mut cron = self.cron.lock().await;
        match cron.remove_job(job_id) {
            Ok(true) => format!("✅ Cancelled task '{}'", job_id),
            Ok(false) => format!("⚠️ No task found with ID '{}'", job_id),
            Err(e) => format!("Error cancelling task: {}", e),
        }
    }
}

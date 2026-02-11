//! Cron service for scheduling agent tasks.
//!
//! Supports both cron expressions (`0 9 * * *`) and interval-based
//! scheduling (every N seconds).

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

/// How a job is scheduled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Schedule {
    /// Cron expression (e.g., "0 9 * * *").
    #[serde(rename = "cron")]
    Cron { expression: String },
    /// Run every N seconds.
    #[serde(rename = "interval")]
    Interval { seconds: u64 },
}

/// A scheduled job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
    pub message: String,
    pub enabled: bool,
    pub created_at: String,
    #[serde(default)]
    pub last_run: Option<String>,
    #[serde(default)]
    pub next_run_ms: Option<i64>,
}

/// Persistent store for cron jobs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CronStore {
    jobs: Vec<CronJob>,
}

pub struct CronService {
    store_path: PathBuf,
    store: CronStore,
}

impl CronService {
    pub fn new(workspace: &Path) -> Self {
        let store_path = workspace.join("cron.json");
        let store = Self::load_store(&store_path);

        Self { store_path, store }
    }

    /// Add a new cron job.
    pub fn add_job(
        &mut self,
        name: &str,
        schedule: Schedule,
        message: &str,
    ) -> anyhow::Result<String> {
        let id = format!("job_{}", uuid_simple());

        // Validate cron expression if applicable
        if let Schedule::Cron { ref expression } = schedule {
            use std::str::FromStr;
            cron::Schedule::from_str(expression)
                .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", expression, e))?;
        }

        let job = CronJob {
            id: id.clone(),
            name: name.to_string(),
            schedule,
            message: message.to_string(),
            enabled: true,
            created_at: Local::now().to_rfc3339(),
            last_run: None,
            next_run_ms: None,
        };

        info!(id = %id, name = name, "Added cron job");
        self.store.jobs.push(job);
        self.save_store()?;

        Ok(id)
    }

    /// Remove a job by ID.
    pub fn remove_job(&mut self, job_id: &str) -> anyhow::Result<bool> {
        let before = self.store.jobs.len();
        self.store.jobs.retain(|j| j.id != job_id);
        let removed = self.store.jobs.len() < before;

        if removed {
            self.save_store()?;
            info!(id = job_id, "Removed cron job");
        }

        Ok(removed)
    }

    /// Enable or disable a job.
    pub fn enable_job(&mut self, job_id: &str, enabled: bool) -> anyhow::Result<bool> {
        if let Some(job) = self.store.jobs.iter_mut().find(|j| j.id == job_id) {
            job.enabled = enabled;
            self.save_store()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// List all jobs.
    pub fn list_jobs(&self, include_disabled: bool) -> Vec<&CronJob> {
        self.store
            .jobs
            .iter()
            .filter(|j| include_disabled || j.enabled)
            .collect()
    }

    /// Get a formatted status string.
    pub fn status(&self) -> String {
        let total = self.store.jobs.len();
        let enabled = self.store.jobs.iter().filter(|j| j.enabled).count();
        format!("{} jobs ({} enabled)", total, enabled)
    }

    /// Get all due jobs (jobs whose next_run_ms <= now).
    pub fn get_due_jobs(&mut self) -> Vec<CronJob> {
        let now_ms = Local::now().timestamp_millis();
        let mut due = Vec::new();

        for job in &mut self.store.jobs {
            if !job.enabled {
                continue;
            }

            let is_due = match job.next_run_ms {
                Some(next) => now_ms >= next,
                None => true, // Never run before
            };

            if is_due {
                job.last_run = Some(Local::now().to_rfc3339());
                job.next_run_ms = Some(compute_next_run(&job.schedule, now_ms));
                due.push(job.clone());
            }
        }

        if !due.is_empty() {
            let _ = self.save_store();
        }

        due
    }

    // ── Private helpers ─────────────────────────────────────────────

    fn load_store(path: &Path) -> CronStore {
        if path.exists() {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            CronStore::default()
        }
    }

    fn save_store(&self) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&self.store)?;
        std::fs::write(&self.store_path, json)?;
        Ok(())
    }
}

/// Compute the next run time in milliseconds.
fn compute_next_run(schedule: &Schedule, now_ms: i64) -> i64 {
    match schedule {
        Schedule::Interval { seconds } => now_ms + (*seconds as i64 * 1000),
        Schedule::Cron { expression } => {
            use std::str::FromStr;
            match cron::Schedule::from_str(expression) {
                Ok(sched) => {
                    let _now = Local::now();
                    sched
                        .upcoming(Local)
                        .next()
                        .map(|dt| dt.timestamp_millis())
                        .unwrap_or(now_ms + 60_000)
                }
                Err(_) => now_ms + 60_000,
            }
        }
    }
}

/// Generate a simple unique ID (no uuid crate dependency).
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_list_jobs() {
        let tmp = std::env::temp_dir().join("ferrobot_test_cron");
        let _ = std::fs::create_dir_all(&tmp);

        let mut service = CronService::new(&tmp);
        let id = service
            .add_job(
                "test-job",
                Schedule::Interval { seconds: 3600 },
                "Check the weather",
            )
            .unwrap();

        let jobs = service.list_jobs(false);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "test-job");

        service.remove_job(&id).unwrap();
        assert!(service.list_jobs(false).is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

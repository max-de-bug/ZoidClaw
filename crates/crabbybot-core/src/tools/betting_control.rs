//! Telegram-accessible control tool for the autonomous betting engine.
//!
//! Lets users start/stop/status/history the betting engine via chat.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::Tool;
use crate::service::betting::BettingState;

/// Control the autonomous Polymarket betting engine.
pub struct BettingControlTool {
    state: Arc<Mutex<BettingState>>,
}

impl BettingControlTool {
    pub fn new(state: Arc<Mutex<BettingState>>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for BettingControlTool {
    fn name(&self) -> &str {
        "betting_control"
    }

    fn description(&self) -> &str {
        "Control the autonomous Polymarket betting engine. \
         Actions: 'start' (resume scanning/trading), 'stop' (pause), \
         'status' (show PnL, positions, config), 'history' (recent trades)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "stop", "status", "history"],
                    "description": "Action to perform on the betting engine"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("status");

        match action {
            "start" => {
                let mut s = self.state.lock().await;
                if s.running {
                    "⚠️ Betting engine is already running.".into()
                } else {
                    s.running = true;
                    format!("🟢 Betting engine **started**!\n\n{}", s.status_report())
                }
            }
            "stop" => {
                let mut s = self.state.lock().await;
                if !s.running {
                    "⚠️ Betting engine is already paused.".into()
                } else {
                    s.running = false;
                    "🔴 Betting engine **stopped**. No new bets will be placed.".into()
                }
            }
            "status" => {
                let s = self.state.lock().await;
                format!("📊 **Betting Engine Status**\n\n{}", s.status_report())
            }
            "history" => {
                let s = self.state.lock().await;
                format!("📜 **Trade History** (last 20)\n\n{}", s.history_report())
            }
            _ => format!("❌ Unknown action '{}'. Use: start, stop, status, history", action),
        }
    }
}

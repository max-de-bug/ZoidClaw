use super::Tool;
use crate::bus::events::{InternalMessage, StreamAction};
use crate::bus::MessageBus;
use crate::service::pumpfun_stream::StreamState;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DiscoveryTool {
    bus: Arc<MessageBus>,
    state: Arc<Mutex<StreamState>>,
}

impl DiscoveryTool {
    pub fn new(bus: Arc<MessageBus>, state: Arc<Mutex<StreamState>>) -> Self {
        Self { bus, state }
    }
}

#[async_trait]
impl Tool for DiscoveryTool {
    fn name(&self) -> &str {
        "discovery"
    }

    fn description(&self) -> &str {
        "Manage the real-time token discovery stream. \
         Use 'start' to begin receiving live Pump.fun token alerts, \
         'stop' to pause the stream, and 'status' to check if it is currently active."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "stop", "status"],
                    "description": "What to do with the stream."
                },
                "chat_id": {
                    "type": "string",
                    "description": "The telegram chat ID or 'cli:direct'. Look for 'Chat ID' in the Identity section of the system prompt."
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

        let chat_id = args
            .get("chat_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        match action {
            "start" => {
                let Some(id) = chat_id else {
                    return "❌ Error: 'chat_id' is required to start the stream. Check the Identity section for your Chat ID.".into();
                };

                // Check if already running
                {
                    let state = self.state.lock().await;
                    if state.worker.is_some() {
                        return format!("📡 **Discovery Stream is ALREADY ACTIVE.** Alerts are currently being sent to chat `{}`.", state.active_chat_id.as_deref().unwrap_or("unknown"));
                    }
                }

                self.bus
                    .publish_internal(InternalMessage::StreamControl {
                        action: StreamAction::Start,
                        chat_id: id.clone(),
                    })
                    .await;
                format!("📡 **Discovery Stream Started!**\nReal-time notifications for new Pump.fun tokens will now be sent to chat `{}`.", id)
            }
            "stop" => {
                let id = chat_id.unwrap_or_default();
                self.bus
                    .publish_internal(InternalMessage::StreamControl {
                        action: StreamAction::Stop,
                        chat_id: id,
                    })
                    .await;
                "🛑 **Discovery Stream Stopped.**\nNotifications have been paused.".to_string()
            }
            "status" => {
                let state = self.state.lock().await;
                if state.worker.is_some() {
                    let id = state.active_chat_id.as_deref().unwrap_or("unknown");
                    format!("📡 **Discovery Stream Status: ACTIVE**\nNotifications are currently being sent to chat `{}`.", id)
                } else {
                    "🌑 **Discovery Stream Status: INACTIVE**\nUse `start discovery` to begin receiving live Pump.fun alerts.".to_string()
                }
            }
            _ => "❌ Error: Invalid action. Use 'start', 'stop', or 'status'.".into(),
        }
    }
}

use super::Tool;
use crate::bus::events::{InternalMessage, StreamAction};
use crate::bus::MessageBus;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub struct DiscoveryTool {
    bus: Arc<MessageBus>,
}

impl DiscoveryTool {
    pub fn new(bus: Arc<MessageBus>) -> Self {
        Self { bus }
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
         and 'stop' to pause the stream."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "stop"],
                    "description": "What to do with the stream."
                },
                "chat_id": {
                    "type": "string",
                    "description": "The chat ID where notifications should be sent (usually provided in context)."
                }
            },
            "required": ["action", "chat_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("stop");
        let chat_id = match args.get("chat_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return "❌ Error: 'chat_id' is required to target the stream.".into(),
        };

        match action {
            "start" => {
                self.bus.publish_internal(InternalMessage::StreamControl {
                    action: StreamAction::Start,
                    chat_id: chat_id.clone(),
                }).await;
                "📡 **Discovery Stream Started!**\nReal-time notifications for new Pump.fun tokens will now be sent to this chat.".to_string()
            }
            "stop" => {
                self.bus.publish_internal(InternalMessage::StreamControl {
                    action: StreamAction::Stop,
                    chat_id: chat_id.clone(),
                }).await;
                "🛑 **Discovery Stream Stopped.**\nNotifications have been paused.".to_string()
            }
            _ => "❌ Error: Invalid action. Use 'start' or 'stop'.".into(),
        }
    }
}

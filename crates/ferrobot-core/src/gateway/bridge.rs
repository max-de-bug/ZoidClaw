use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};
use crate::agent::AgentLoop;
use crate::bus::MessageBus;

/// Bridges the asynchronous MessageBus with the synchronous AgentLoop.
///
/// It listens for `InboundMessage`s from the bus, processes them through
/// the agent, and publishes the resulting `OutboundMessage`s.
pub struct AgentBridge {
    bus: Arc<Mutex<MessageBus>>,
    agent: AgentLoop,
}

impl AgentBridge {
    pub fn new(bus: Arc<Mutex<MessageBus>>, agent: AgentLoop) -> Self {
        Self { bus, agent }
    }

    /// Run the bridge loop until the bus is closed.
    pub async fn run(mut self, mut inbound_rx: tokio::sync::mpsc::Receiver<crate::bus::events::InboundMessage>) -> Result<()> {
        info!("Agent bridge started, waiting for inbound messages...");

        while let Some(msg) = inbound_rx.recv().await {
            debug!(
                channel = msg.channel,
                chat_id = msg.chat_id,
                "Bridge received message"
            );

            let session_key = format!("{}:{}", msg.channel, msg.chat_id);
            
            match self.agent.process(&msg.content, &session_key).await {
                Ok(response) => {
                    let bus = self.bus.lock().await;
                    bus.publish_outbound(crate::bus::events::OutboundMessage {
                        channel: msg.channel,
                        chat_id: msg.chat_id,
                        content: response,
                    }).await;
                }
                Err(e) => {
                    error!("Error processing message through agent: {}", e);
                    
                    let bus = self.bus.lock().await;
                    bus.publish_outbound(crate::bus::events::OutboundMessage {
                        channel: msg.channel,
                        chat_id: msg.chat_id,
                        content: format!("⚠️ Error: {}", e),
                    }).await;
                }
            }
        }

        info!("Agent bridge shutting down (bus closed)");
        Ok(())
    }
}

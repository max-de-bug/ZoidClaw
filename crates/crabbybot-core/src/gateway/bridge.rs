use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use crate::agent::AgentLoop;
use crate::bus::MessageBus;

/// Bridges the asynchronous MessageBus with the synchronous AgentLoop.
///
/// It listens for `InboundMessage`s from the bus, processes them through
/// the agent, and publishes the resulting `OutboundMessage`s.
///
/// The bridge supports graceful shutdown via a [`CancellationToken`]. When
/// the token is cancelled, it finishes processing any in-flight message
/// before exiting.
pub struct AgentBridge {
    bus: Arc<Mutex<MessageBus>>,
    agent: AgentLoop,
    cancel: CancellationToken,
}

impl AgentBridge {
    pub fn new(bus: Arc<Mutex<MessageBus>>, agent: AgentLoop, cancel: CancellationToken) -> Self {
        Self { bus, agent, cancel }
    }

    /// Run the bridge loop until the bus is closed or cancellation is requested.
    pub async fn run(mut self, mut inbound_rx: tokio::sync::mpsc::Receiver<crate::bus::events::InboundMessage>) -> Result<()> {
        info!("Agent bridge started, waiting for inbound messages...");

        loop {
            tokio::select! {
                // Cancellation branch — stop accepting new messages.
                _ = self.cancel.cancelled() => {
                    info!("Agent bridge received shutdown signal");
                    break;
                }
                // Normal message processing branch.
                msg = inbound_rx.recv() => {
                    match msg {
                        Some(msg) => {
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

                                    let error_msg = if e.to_string().contains("429") || e.to_string().contains("quota") || e.to_string().contains("exhausted") {
                                        "⚠️ **LLM Quota Exceeded**\n\nAll configured providers (OpenAI, Gemini) have exhausted their free-tier quotas or hit rate limits. \n\n**Suggestions:**\n1. Wait a few minutes for rate limits to reset.\n2. Add a **Groq** API key to your `config.json` for a generous free tier.\n3. Check your billing details for OpenAI/Gemini.".into()
                                    } else {
                                        format!("⚠️ **Error**: {}", e)
                                    };

                                    let bus = self.bus.lock().await;
                                    bus.publish_outbound(crate::bus::events::OutboundMessage {
                                        channel: msg.channel,
                                        chat_id: msg.chat_id,
                                        content: error_msg,
                                    }).await;
                                }
                            }
                        }
                        None => {
                            // Channel closed — all senders dropped.
                            break;
                        }
                    }
                }
            }
        }

        info!("Agent bridge shutting down gracefully");
        Ok(())
    }
}

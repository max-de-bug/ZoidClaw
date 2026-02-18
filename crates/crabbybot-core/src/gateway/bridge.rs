use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use crate::agent::AgentLoop;
use crate::bus::MessageBus;
use crate::cron::CronService;
use crate::session::SessionManager;

/// Bridges the asynchronous MessageBus with the synchronous AgentLoop.
///
/// It listens for `InboundMessage`s from the bus, processes them through
/// the agent, and publishes the resulting `OutboundMessage`s.
///
/// The bridge supports:
/// - **Command routing**: `/help`, `/status`, `/clear` are handled directly
/// - **Agent passthrough**: all other messages go to the LLM
/// - **Graceful shutdown** via a [`CancellationToken`]
pub struct AgentBridge {
    bus: Arc<MessageBus>,
    agent: AgentLoop,
    cancel: CancellationToken,
    cron: Arc<Mutex<CronService>>,
    workspace: PathBuf,
    start_time: std::time::Instant,
}

impl AgentBridge {
    pub fn new(
        bus: Arc<MessageBus>,
        agent: AgentLoop,
        cancel: CancellationToken,
        cron: Arc<Mutex<CronService>>,
        workspace: PathBuf,
    ) -> Self {
        Self {
            bus,
            agent,
            cancel,
            cron,
            workspace,
            start_time: std::time::Instant::now(),
        }
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

                            // Check for slash commands first (only from real users, not system/cron).
                            if !msg.is_system {
                                if let Some(response) = self.handle_command(&msg.content, &session_key).await {
                                    self.bus.publish_outbound(crate::bus::events::OutboundMessage {
                                        channel: msg.channel,
                                        chat_id: msg.chat_id,
                                        content: response,
                                    }).await;
                                    continue;
                                }
                            }

                            // Normal agent processing.
                            match self.agent.process(&msg.content, &session_key).await {
                                Ok(response) => {
                                    self.bus.publish_outbound(crate::bus::events::OutboundMessage {
                                        channel: msg.channel,
                                        chat_id: msg.chat_id,
                                        content: response,
                                    }).await;
                                }
                                Err(e) => {
                                    error!("Error processing message through agent: {}", e);

                                    let error_msg = if e.to_string().contains("429") || e.to_string().contains("quota") || e.to_string().contains("exhausted") {
                                        "⚠️ **LLM Quota Exceeded**\n\nAll configured providers have exhausted their quotas or hit rate limits. \n\n**Suggestions:**\n1. Wait a few minutes for rate limits to reset.\n2. Add a **Groq** API key for a generous free tier.\n3. Check your billing details.".into()
                                    } else {
                                        format!("⚠️ **Error**: {}", e)
                                    };
                                    
                                    self.bus.publish_outbound(crate::bus::events::OutboundMessage {
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

    // ── Command Routing ─────────────────────────────────────────────

    /// Handle slash commands. Returns `Some(response)` if the message was
    /// a command, `None` if it should be passed to the agent.
    async fn handle_command(&self, content: &str, session_key: &str) -> Option<String> {
        let trimmed = content.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        let (cmd, _args) = trimmed
            .split_once(' ')
            .unwrap_or((trimmed, ""));

        match cmd {
            "/help" => Some(self.cmd_help()),
            "/status" => Some(self.cmd_status().await),
            "/clear" => Some(self.cmd_clear(session_key)),
            "/start" => Some(self.cmd_help()), // Telegram sends /start on first use
            _ => None, // Unknown commands pass through to the agent
        }
    }

    fn cmd_help(&self) -> String {
        "🦀 **Crabbybot Commands**\n\n\
         `/help` — Show this help message\n\
         `/status` — Bot status (providers, model, uptime)\n\
         `/clear` — Clear conversation history\n\n\
         **Scheduling** (via natural language):\n\
         Just ask! e.g. *\"Remind me to check SOL price every hour\"*\n\n\
         **Solana** (via natural language):\n\
         *\"What's the SOL balance of [address]?\"*\n\
         *\"Show recent transactions for [address]\"*\n\
         *\"What tokens does [address] hold?\"*\n\n\
         Any other message is processed by the AI assistant."
            .to_string()
    }

    async fn cmd_status(&self) -> String {
        let uptime = self.start_time.elapsed();
        let hours = uptime.as_secs() / 3600;
        let mins = (uptime.as_secs() % 3600) / 60;
        let secs = uptime.as_secs() % 60;

        let cron = self.cron.lock().await;
        let cron_status = cron.status();

        format!(
            "🤖 **Crabbybot Status**\n\n\
             ⏱ Uptime: {}h {}m {}s\n\
             📋 Cron: {}\n\
             📂 Workspace: `{}`",
            hours,
            mins,
            secs,
            cron_status,
            self.workspace.display(),
        )
    }

    fn cmd_clear(&self, session_key: &str) -> String {
        let mut mgr = SessionManager::new(&self.workspace);
        if mgr.delete(session_key) {
            "✅ Conversation history cleared.".to_string()
        } else {
            "ℹ️ No conversation history to clear.".to_string()
        }
    }
}

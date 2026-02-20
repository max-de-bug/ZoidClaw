use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::agent::{AgentError, AgentLoop};
use crate::bus::events::OutboundMessage;
use crate::bus::MessageBus;
use crate::cron::CronService;
use crate::session::SessionManager;

/// Bridges the asynchronous [`MessageBus`] with the [`AgentLoop`].
///
/// It listens for `InboundMessage`s from the bus, processes them through
/// the agent, and publishes the resulting `OutboundMessage`s.
///
/// ## Concurrency model
///
/// The agent is shared as `Arc<Mutex<AgentLoop>>`. Each inbound message is
/// handled in its own `tokio::spawn`'d task, so messages from *different*
/// chat sessions are processed concurrently without blocking each other.
/// Because the `Mutex` serialises LLM calls globally, this is safe but not
/// fully parallel across sessions — a good starting point that can be
/// upgraded to a per-session pool later.
///
/// ## What the bridge handles
/// - **Command routing**: `/help`, `/status`, `/clear` are handled directly.
/// - **Agent passthrough**: all other messages go to the LLM.
/// - **Streaming events**: `Typing` and `Progress` are forwarded to the bus
///   by the agent loop itself.
/// - **Graceful shutdown** via a [`CancellationToken`].
pub struct AgentBridge {
    bus: Arc<MessageBus>,
    agent: Arc<Mutex<AgentLoop>>,
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
            agent: Arc::new(Mutex::new(agent)),
            cancel,
            cron,
            workspace,
            start_time: std::time::Instant::now(),
        }
    }

    /// Run the bridge loop until the bus is closed or cancellation is requested.
    pub async fn run(
        self,
        mut inbound_rx: tokio::sync::mpsc::Receiver<crate::bus::events::InboundMessage>,
    ) -> Result<()> {
        info!("Agent bridge started, waiting for inbound messages…");

        let Self { bus, agent, cancel, cron, workspace, start_time } = self;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Agent bridge received shutdown signal");
                    break;
                }
                msg = inbound_rx.recv() => {
                    match msg {
                        None => {
                            // All inbound_tx senders dropped — shut down.
                            break;
                        }
                        Some(msg) => {
                            debug!(
                                channel = msg.channel,
                                chat_id = msg.chat_id,
                                "Bridge received message"
                            );

                            // Clone the cheap Arcs to move into the spawned task.
                            let bus_t      = Arc::clone(&bus);
                            let agent_t    = Arc::clone(&agent);
                            let cron_t     = Arc::clone(&cron);
                            let workspace_t = workspace.clone();
                            let channel    = msg.channel.clone();
                            let chat_id    = msg.chat_id.clone();
                            let session_key = format!("{}:{}", channel, chat_id);
                            let content    = msg.content.clone();
                            let is_system  = msg.is_system;

                            tokio::spawn(async move {
                                // ── Command routing (non-system messages only) ──────
                                if !is_system {
                                    if let Some(response) = handle_command(
                                        &content,
                                        &session_key,
                                        &cron_t,
                                        &workspace_t,
                                        start_time,
                                    )
                                    .await
                                    {
                                        bus_t
                                            .publish_outbound(OutboundMessage::reply(
                                                &channel, &chat_id, response,
                                            ))
                                            .await;
                                        return;
                                    }
                                }

                                // ── Agent processing ───────────────────────────────
                                let result = {
                                    let mut lock = agent_t.lock().await;
                                    lock.process(&content, &session_key, Some(&bus_t)).await
                                };

                                match result {
                                    Ok(reply) => {
                                        bus_t
                                            .publish_outbound(OutboundMessage::reply(
                                                &channel, &chat_id, reply,
                                            ))
                                            .await;
                                    }
                                    Err(e) => {
                                        error!("Error processing message: {}", e);
                                        let error_msg = format_agent_error(&e);
                                        bus_t
                                            .publish_outbound(OutboundMessage::reply(
                                                &channel, &chat_id, error_msg,
                                            ))
                                            .await;
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }

        info!("Agent bridge shutting down gracefully");
        Ok(())
    }
}

// ── Command routing ───────────────────────────────────────────────────────────

/// Handle slash commands. Returns `Some(response)` if the message was a
/// recognised command, `None` if the message should pass to the agent.
async fn handle_command(
    content: &str,
    session_key: &str,
    cron: &Arc<Mutex<CronService>>,
    workspace: &PathBuf,
    start_time: std::time::Instant,
) -> Option<String> {
    let trimmed = content.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let (cmd, _args) = trimmed.split_once(' ').unwrap_or((trimmed, ""));

    match cmd {
        "/help" | "/start" => Some(cmd_help()),
        "/status" => Some(cmd_status(cron, workspace, start_time).await),
        "/clear" => Some(cmd_clear(session_key, workspace)),
        _ => None, // Unknown commands pass through to the agent
    }
}

fn cmd_help() -> String {
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

async fn cmd_status(
    cron: &Arc<Mutex<CronService>>,
    workspace: &PathBuf,
    start_time: std::time::Instant,
) -> String {
    let uptime = start_time.elapsed();
    let hours = uptime.as_secs() / 3600;
    let mins = (uptime.as_secs() % 3600) / 60;
    let secs = uptime.as_secs() % 60;

    let cron = cron.lock().await;
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
        workspace.display(),
    )
}

fn cmd_clear(session_key: &str, workspace: &PathBuf) -> String {
    let mut mgr = SessionManager::new(workspace);
    if mgr.delete(session_key) {
        "✅ Conversation history cleared.".to_string()
    } else {
        "ℹ️ No conversation history to clear.".to_string()
    }
}

// ── Error formatting ──────────────────────────────────────────────────────────

/// Convert an [`AgentError`] into a user-facing Markdown string.
fn format_agent_error(e: &AgentError) -> String {
    match e {
        AgentError::MaxIterationsExceeded(n) => {
            format!(
                "⚠️ **Max iterations reached** ({n})\n\n\
                 The agent completed {n} tool-call rounds without a final answer. \
                 Try a simpler request or increase `max_tool_iterations` in your config."
            )
        }
        AgentError::Provider(inner) => {
            let msg = inner.to_string();
            if msg.contains("429")
                || msg.contains("quota")
                || msg.contains("exhausted")
                || msg.contains("rate_limit")
            {
                "⚠️ **LLM Quota / Rate-limit**\n\n\
                 All configured providers have hit their limits.\n\n\
                 **Options:**\n\
                 1. Wait a few minutes for rate limits to reset.\n\
                 2. Add a **Groq** API key for a generous free tier.\n\
                 3. Check your billing details."
                    .into()
            } else {
                format!("⚠️ **Provider error**: {}", inner)
            }
        }
        AgentError::Session(inner) => {
            format!("⚠️ **Session error**: {}", inner)
        }
    }
}

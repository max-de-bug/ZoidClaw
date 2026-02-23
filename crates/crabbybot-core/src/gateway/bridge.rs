use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::agent::{AgentError, AgentLoop};
use crate::bus::events::{InboundMessage, OutboundMessage};
use crate::bus::MessageBus;
use crate::cron::CronService;

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
        mut inbound_rx: mpsc::Receiver<InboundMessage>,
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
                                    match handle_command(
                                        &content,
                                        &session_key,
                                        &cron_t,
                                        &workspace_t,
                                        start_time,
                                        &agent_t,
                                    )
                                    .await
                                    {
                                        Some(CommandResult::Reply(response)) => {
                                            bus_t
                                                .publish_outbound(OutboundMessage::reply(
                                                    &channel, &chat_id, response,
                                                ))
                                                .await;
                                            return;
                                        }
                                        Some(CommandResult::AgentPassthrough(prompt)) => {
                                            // Rewrite the command into a natural language prompt
                                            // and fall through to agent processing below.
                                            let result = {
                                                let mut lock = agent_t.lock().await;
                                                lock.process(&prompt, &session_key, Some(&bus_t)).await
                                            };
                                            match result {
                                                Ok(res) => {
                                                    let outbound = if let Some(btns) = res.buttons {
                                                        OutboundMessage::reply_with_buttons(&channel, &chat_id, res.content, btns)
                                                    } else {
                                                        OutboundMessage::reply(&channel, &chat_id, res.content)
                                                    };
                                                    bus_t.publish_outbound(outbound).await;
                                                }
                                                Err(e) => {
                                                    error!("Error processing command passthrough: {}", e);
                                                    let error_msg = format_agent_error(&e);
                                                    bus_t
                                                        .publish_outbound(OutboundMessage::reply(
                                                            &channel, &chat_id, error_msg,
                                                        ))
                                                        .await;
                                                }
                                            }
                                            return;
                                        }
                                        None => {} // Not a command, fall through to agent
                                    }
                                }

                                // ── Agent processing ───────────────────────────────
                                let result = {
                                    let mut lock = agent_t.lock().await;
                                    lock.process(&content, &session_key, Some(&bus_t)).await
                                };

                                match result {
                                    Ok(res) => {
                                        let outbound = if let Some(btns) = res.buttons {
                                            OutboundMessage::reply_with_buttons(&channel, &chat_id, res.content, btns)
                                        } else {
                                            OutboundMessage::reply(&channel, &chat_id, res.content)
                                        };
                                        bus_t.publish_outbound(outbound).await;
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

/// Result of command routing — either a direct reply or a prompt to pipe
/// through the agent loop.
enum CommandResult {
    /// Send this text directly to the user.
    Reply(String),
    /// Rewrite the command into this prompt and process via `AgentLoop`.
    AgentPassthrough(String),
}

/// Handle slash commands. Returns `Some(CommandResult)` if the message was a
/// recognised command, `None` if the message should pass to the agent as-is.
async fn handle_command(
    content: &str,
    session_key: &str,
    cron: &Arc<Mutex<CronService>>,
    workspace: &Path,
    start_time: std::time::Instant,
    agent: &Arc<Mutex<AgentLoop>>,
) -> Option<CommandResult> {
    let trimmed = content.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let (cmd, args) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
    let args = args.trim();

    match cmd {
        "/help" | "/start" => Some(CommandResult::Reply(cmd_help())),
        "/status" => Some(CommandResult::Reply(cmd_status(cron, workspace, start_time).await)),
        "/clear" | "/reset" | "/forget" => Some(CommandResult::Reply(cmd_clear(session_key, agent).await)),
        // Crypto shortcuts — rewrite into agent prompts
        "/portfolio" => Some(CommandResult::AgentPassthrough(
            "Show my Solana wallet portfolio: SOL balance and all token balances.".into(),
        )),
        "/alpha" if !args.is_empty() => Some(CommandResult::AgentPassthrough(
            format!("Give me a full alpha summary for token {}", args),
        )),
        "/buy" if !args.is_empty() => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            let mint = parts[0];
            let amount = parts.get(1).unwrap_or(&"0.1");
            Some(CommandResult::AgentPassthrough(
                format!("Buy {} SOL of token {}", amount, mint),
            ))
        }
        _ => None,
    }
}

fn cmd_help() -> String {
    "🦀 **Crabbybot Commands**\n\n\
     🛠️ **General:**\n\
     `/help` — Show this help message\n\
     `/status` — Bot status (providers, model, uptime)\n\
     `/clear` (or `/reset`, `/forget`) — Clear conversation history\n\n\
     💰 **Crypto Shortcuts:**\n\
     `/portfolio` — Your wallet’s SOL + token balances\n\
     `/alpha <mint>` — Full safety + sentiment report\n\
     `/buy <mint> [amount]` — Buy token (default: 0.1 SOL)\n\n\
     ⏰ **Scheduling:**\n\
     Just ask! e.g. *\"Remind me to check SOL price every hour\"*\n\n\
     Any other message is processed by the AI assistant."
        .to_string()
}

async fn cmd_status(
    cron: &Arc<Mutex<CronService>>,
    workspace: &Path,
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

async fn cmd_clear(session_key: &str, agent: &Arc<Mutex<AgentLoop>>) -> String {
    let mut lock = agent.lock().await;
    if lock.clear_session(session_key) {
        "✅ Conversation history cleared. I have forgotten our past messages."
            .to_string()
    } else {
        "ℹ️ No conversation history to clear."
            .to_string()
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

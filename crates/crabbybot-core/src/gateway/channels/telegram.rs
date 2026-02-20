use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::MessageId;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use crate::bus::MessageBus;
use crate::bus::events::InboundMessage;
use crate::gateway::utils::chunk_message;

/// Maximum Telegram message length.
const TELEGRAM_MAX_LEN: usize = 4096;

/// Tracks the progress message state for a single chat.
///
/// Instead of sending a new message for each tool invocation, we keep
/// the `MessageId` of the first progress message and **edit** it with
/// accumulated status lines. This produces a single, evolving message
/// that looks professional instead of spamming the chat.
#[derive(Debug, Clone, Default)]
struct ProgressState {
    /// The Telegram message ID of the current progress message.
    message_id: Option<MessageId>,
    /// Accumulated status lines (one per tool-call batch).
    lines: Vec<String>,
}

/// Per-chat progress tracker, shared between the outbound callback closure
/// and the rest of the transport.
type ProgressTracker = Arc<Mutex<HashMap<String, ProgressState>>>;

pub struct TelegramTransport {
    token: String,
    bus: Arc<MessageBus>,
    allow_from: Vec<String>,
}

impl TelegramTransport {
    pub fn new(
        token: String,
        bus: Arc<MessageBus>,
        allow_from: Vec<String>,
    ) -> Self {
        Self { token, bus, allow_from }
    }

    pub async fn run(self) -> Result<()> {
        let bot = Bot::new(&self.token);
        let progress: ProgressTracker = Arc::new(Mutex::new(HashMap::new()));

        info!("Telegram transport started");

        // Subscribe to outbound messages FIRST (before dispatcher starts)
        {
            let bot_out = bot.clone();
            let progress_out = Arc::clone(&progress);

            self.bus.subscribe_outbound("telegram", move |msg| {
                use crate::bus::events::OutboundMessage;
                let bot_out = bot_out.clone();
                let progress_out = Arc::clone(&progress_out);

                async move {
                    match msg {
                        OutboundMessage::Reply { chat_id, content, .. } => {
                            // ── Final reply: send as new message(s) and clear progress ──
                            if let Ok(id) = chat_id.parse::<i64>() {
                                let chunks = chunk_message(&content, TELEGRAM_MAX_LEN);
                                for chunk in chunks {
                                    if let Err(e) = bot_out.send_message(ChatId(id), chunk).await {
                                        error!("Failed to send Telegram message: {}", e);
                                    }
                                }
                            }
                            // Clear any accumulated progress for this chat
                            progress_out.lock().await.remove(&chat_id);
                        }

                        OutboundMessage::Progress { chat_id, content, .. } => {
                            // ── Progress: edit-in-place or send first message ──
                            if let Ok(id) = chat_id.parse::<i64>() {
                                let mut tracker = progress_out.lock().await;
                                let state = tracker.entry(chat_id.clone()).or_default();

                                // Append new progress line
                                state.lines.push(content);

                                // Build consolidated message with tree-style formatting
                                let consolidated = format_progress_lines(&state.lines);

                                match state.message_id {
                                    Some(msg_id) => {
                                        // Edit existing progress message
                                        let result = bot_out
                                            .edit_message_text(ChatId(id), msg_id, &consolidated)
                                            .await;
                                        if let Err(e) = result {
                                            debug!(
                                                "Failed to edit progress message, sending new: {}",
                                                e
                                            );
                                            // If editing fails (e.g., message too old), send a new one
                                            match bot_out
                                                .send_message(ChatId(id), &consolidated)
                                                .await
                                            {
                                                Ok(sent) => {
                                                    state.message_id = Some(sent.id);
                                                }
                                                Err(e) => {
                                                    error!("Failed to send progress message: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        // First progress message — send and store its ID
                                        match bot_out
                                            .send_message(ChatId(id), &consolidated)
                                            .await
                                        {
                                            Ok(sent) => {
                                                state.message_id = Some(sent.id);
                                            }
                                            Err(e) => {
                                                error!("Failed to send progress message: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        OutboundMessage::Typing { chat_id, .. } => {
                            if let Ok(id) = chat_id.parse::<i64>() {
                                use teloxide::types::ChatAction;
                                let _ = bot_out
                                    .send_chat_action(ChatId(id), ChatAction::Typing)
                                    .await;
                            }
                        }
                    }
                }
            }).await;
        }

        // Set up inbound update handler
        let bus = Arc::clone(&self.bus);
        let allow_from = self.allow_from.clone();
        let handler = Update::filter_message().endpoint(
            move |_bot: Bot, msg: Message, bus: Arc<MessageBus>, allow_from: Vec<String>| async move {
                let user_id = msg.from.as_ref().map(|u| u.id.to_string()).unwrap_or_else(|| "unknown".to_owned());

                // Enforce allowFrom ACL
                if !allow_from.is_empty() && !allow_from.contains(&user_id) {
                    warn!(
                        user_id = user_id,
                        chat_id = msg.chat.id.to_string(),
                        "Rejected message from user not in allowFrom list"
                    );
                    return respond(());
                }

                if let Some(text) = msg.text() {
                    let inbound = InboundMessage {
                        channel: "telegram".to_owned(),
                        chat_id: msg.chat.id.to_string(),
                        user_id,
                        content: text.to_owned(),
                        media: Vec::new(),
                        is_system: false,
                    };

                    if let Err(e) = bus.inbound_sender().send(inbound).await {
                        error!("Failed to send inbound message to bus: {}", e);
                    }
                }
                respond(())
            },
        );

        Dispatcher::builder(bot, handler)
            .dependencies(dptree::deps![bus, allow_from])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;

        Ok(())
    }
}

/// Formats accumulated progress lines into a clean tree-style view.
///
/// ```text
/// 🔄 Processing your request…
/// ├ 🔍 web_search
/// ├ 🔍 web_search
/// └ 📄 web_fetch
/// ```
fn format_progress_lines(lines: &[String]) -> String {
    let mut out = String::from("🔄 Processing your request…\n");
    let len = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let connector = if i == len - 1 { "└" } else { "├" };
        // Extract the tool name from progress text like "⚙️ Running tool: `web_search`…"
        let display = prettify_tool_line(line);
        out.push_str(&format!("{} {}\n", connector, display));
    }
    out
}

/// Converts a raw progress message into a friendlier display line.
///
/// Input:  `"⚙️ Running tool: `web_search`…"`
/// Output: `"🔍 web_search"`
fn prettify_tool_line(line: &str) -> String {
    // Try to extract tool names from the standard format
    if let Some(rest) = line.strip_prefix("⚙️ Running tool: `") {
        if let Some(name) = rest.strip_suffix("`…") {
            let icon = tool_icon(name);
            return format!("{} {}", icon, name);
        }
    }
    if let Some(rest) = line.strip_prefix("⚙️ Running ") {
        // Multi-tool format: "⚙️ Running 2 tools in parallel: `a`, `b`…"
        return format!("⚙️ {}", rest);
    }
    // Fallback: return as-is
    line.to_string()
}

/// Returns a contextual emoji icon for a tool name.
fn tool_icon(name: &str) -> &'static str {
    match name {
        "web_search" => "🔍",
        "web_fetch" => "📄",
        "shell_exec" | "exec" => "⚡",
        "read_file" => "📖",
        "write_file" => "✏️",
        "list_dir" => "📁",
        _ => "⚙️",
    }
}

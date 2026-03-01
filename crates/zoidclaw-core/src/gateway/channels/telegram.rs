use crate::bus::events::InboundMessage;
use crate::bus::MessageBus;
use crate::gateway::utils::chunk_message;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::MessageId;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

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
    pub fn new(token: String, bus: Arc<MessageBus>, allow_from: Vec<String>) -> Self {
        Self {
            token,
            bus,
            allow_from,
        }
    }

    pub async fn run(self) -> Result<()> {
        let bot = Bot::new(&self.token);
        let progress: ProgressTracker = Arc::new(Mutex::new(HashMap::new()));

        info!("Telegram transport started");

        // Ensure no webhooks are active and drop pending updates before starting polling.
        // This prevents the common `Api(TerminatedByOtherGetUpdates)` error if a webhook
        // was previously configured on this bot token.
        if let Err(e) = bot.delete_webhook().drop_pending_updates(true).send().await {
            warn!("Failed to delete webhook (normal on first startup): {}", e);
        }

        // Subscribe to outbound messages FIRST (before dispatcher starts)
        {
            let bot_out = bot.clone();
            let progress_out = Arc::clone(&progress);

            self.bus
                .subscribe_outbound("telegram", move |msg| {
                    use crate::bus::events::OutboundMessage;
                    let bot_out = bot_out.clone();
                    let progress_out = Arc::clone(&progress_out);

                    async move {
                        match msg {
                            OutboundMessage::Reply {
                                chat_id,
                                content,
                                buttons,
                                ..
                            } => {
                                // ── Final reply: send as new message(s) and clear progress ──
                                if let Ok(id) = chat_id.parse::<i64>() {
                                    let chunks = chunk_message(&content, TELEGRAM_MAX_LEN);
                                    let num_chunks = chunks.len();

                                    for (i, chunk) in chunks.into_iter().enumerate() {
                                        let mut send = bot_out.send_message(ChatId(id), chunk);

                                        // Attach buttons only to the LAST chunk
                                        if i == num_chunks - 1 {
                                            if let Some(ref btns) = buttons {
                                                use teloxide::types::{
                                                    InlineKeyboardButton, InlineKeyboardMarkup,
                                                };
                                                let keyboard: Vec<Vec<InlineKeyboardButton>> = btns
                                                    .iter()
                                                    .map(|b| {
                                                        let btn = if let Some(ref url) = b.url {
                                                            InlineKeyboardButton::url(
                                                                b.text.clone(),
                                                                url.parse().unwrap_or(
                                                                    "https://google.com"
                                                                        .parse()
                                                                        .unwrap(),
                                                                ),
                                                            )
                                                        } else {
                                                            InlineKeyboardButton::callback(
                                                                b.text.clone(),
                                                                b.data.clone().unwrap_or_default(),
                                                            )
                                                        };
                                                        vec![btn]
                                                    })
                                                    .collect();
                                                send = send.reply_markup(
                                                    InlineKeyboardMarkup::new(keyboard),
                                                );
                                            }
                                        }

                                        if let Err(e) = send.await {
                                            error!("Failed to send Telegram message: {}", e);
                                        }
                                    }
                                }
                                // Clear any accumulated progress for this chat
                                progress_out.lock().await.remove(&chat_id);
                            }

                            OutboundMessage::Progress {
                                chat_id, content, ..
                            } => {
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
                                                .edit_message_text(
                                                    ChatId(id),
                                                    msg_id,
                                                    &consolidated,
                                                )
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
                                                        error!(
                                                            "Failed to send progress message: {}",
                                                            e
                                                        );
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
                                                    error!(
                                                        "Failed to send progress message: {}",
                                                        e
                                                    );
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
                })
                .await;
        }

        // Set up inbound update handler
        let bus = Arc::clone(&self.bus);
        let allow_from = self.allow_from.clone();

        let message_handler = Update::filter_message().endpoint(
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
                    let normalized = text.trim();
                    let lower = normalized.to_lowercase();

                    // ── FAST PATH: /config command (bypass LLM) ──
                    if lower == "/config" || lower == "config"
                        || lower.starts_with("/config ") || lower.starts_with("config ")
                    {
                        let args_str = if lower == "/config" || lower == "config" {
                            ""
                        } else if normalized.starts_with('/') {
                            normalized[8..].trim() // skip "/config "
                        } else {
                            normalized[7..].trim() // skip "config "
                        };
                        let args_lower = args_str.to_lowercase();

                        use crate::config::Config;
                        let mut config = Config::load().unwrap_or_default();

                        if args_str.is_empty() || args_lower == "help" || args_lower == "--help" {
                            // Show current config summary with masked keys
                            let mask = |s: &str| -> String {
                                if s.is_empty() || s.contains("YOUR_") { return "❌ not set".into(); }
                                if s.len() <= 8 { return "••••••••".into(); }
                                format!("{}••••{}", &s[..4], &s[s.len()-4..])
                            };

                            let groq_key = config.providers.groq.as_ref().map(|p| mask(&p.api_key)).unwrap_or("❌ not set".into());
                            let openai_key = config.providers.openai.as_ref().map(|p| mask(&p.api_key)).unwrap_or("❌ not set".into());
                            let anthropic_key = config.providers.anthropic.as_ref().map(|p| mask(&p.api_key)).unwrap_or("❌ not set".into());
                            let gemini_key = config.providers.gemini.as_ref().map(|p| mask(&p.api_key)).unwrap_or("❌ not set".into());
                            let openrouter_key = config.providers.openrouter.as_ref().map(|p| mask(&p.api_key)).unwrap_or("❌ not set".into());
                            let poly_key = config.tools.polymarket.private_key.as_deref().map(|k| mask(k)).unwrap_or("❌ not set".into());
                            let solana_key = config.tools.solana_private_key.as_deref().map(|k| mask(k)).unwrap_or("❌ not set".into());

                            let summary = format!(
"⚙️ ZoidClaw Configuration

━━━ 🔑 LLM Providers ━━━
Groq: {}
OpenAI: {}
Anthropic: {}
Gemini: {}
OpenRouter: {}

━━━ 🤖 Agent ━━━
Model: {}
Max Tokens: {}

━━━ 🔐 Wallet Keys ━━━
Polymarket: {}
Solana: {}

━━━ 🎰 Betting ━━━
Enabled: {}
Max Bet: ${}
Daily Loss Limit: ${}

━━━ ✏️ Set a value ━━━
/config set groq_key <KEY>
/config set openai_key <KEY>
/config set anthropic_key <KEY>
/config set gemini_key <KEY>
/config set openrouter_key <KEY>
/config set polymarket_key <KEY>
/config set solana_key <KEY>
/config set model <MODEL>
/config set max_bet <AMOUNT>
/config set daily_limit <AMOUNT>",
                                groq_key, openai_key, anthropic_key, gemini_key, openrouter_key,
                                config.agents.defaults.model,
                                config.agents.defaults.max_tokens,
                                poly_key, solana_key,
                                if config.tools.betting.enabled { "🟢" } else { "🔴" },
                                config.tools.betting.max_bet_size_usdc,
                                config.tools.betting.daily_loss_limit_usdc,
                            );
                            let _ = _bot.send_message(msg.chat.id, summary).await;
                            return respond(());
                        }

                        // Handle "set <key> <value>"
                        if args_lower.starts_with("set ") {
                            // ── SECURITY: Delete the user's message immediately ──
                            // API keys should never persist in Telegram chat history.
                            let _ = _bot.delete_message(msg.chat.id, msg.id).await;

                            let set_args = args_str[4..].trim();
                            let parts: Vec<&str> = set_args.splitn(2, ' ').collect();
                            if parts.len() < 2 {
                                let _ = _bot.send_message(msg.chat.id, "❌ Usage: /config set <key> <value>\n🔒 Your message was auto-deleted for security.").await;
                                return respond(());
                            }
                            let key = parts[0].to_lowercase();
                            let value = parts[1].trim().to_string();

                            // Determine if this key holds a sensitive secret
                            let is_secret = matches!(key.as_str(),
                                "groq_key" | "openai_key" | "anthropic_key"
                                | "gemini_key" | "openrouter_key"
                                | "polymarket_key" | "solana_key"
                            );

                            // Encrypt secrets before storing
                            let store_value = if is_secret {
                                match crate::vault::encrypt(&value) {
                                    Ok(encrypted) => encrypted,
                                    Err(e) => {
                                        let _ = _bot.send_message(msg.chat.id, format!("❌ Encryption failed: {}\n🔒 Your message was auto-deleted.", e)).await;
                                        return respond(());
                                    }
                                }
                            } else {
                                value.clone()
                            };

                            let preview = if value.len() > 4 {
                                format!("{}••••", &value[..4])
                            } else {
                                "••••••••".to_string()
                            };

                            let result = match key.as_str() {
                                "groq_key" => {
                                    let entry = config.providers.groq.get_or_insert_with(Default::default);
                                    entry.api_key = store_value;
                                    Ok(format!("Groq API key set ({})", preview))
                                }
                                "openai_key" => {
                                    let entry = config.providers.openai.get_or_insert_with(Default::default);
                                    entry.api_key = store_value;
                                    Ok(format!("OpenAI API key set ({})", preview))
                                }
                                "anthropic_key" => {
                                    let entry = config.providers.anthropic.get_or_insert_with(Default::default);
                                    entry.api_key = store_value;
                                    Ok(format!("Anthropic API key set ({})", preview))
                                }
                                "gemini_key" => {
                                    let entry = config.providers.gemini.get_or_insert_with(Default::default);
                                    entry.api_key = store_value;
                                    Ok(format!("Gemini API key set ({})", preview))
                                }
                                "openrouter_key" => {
                                    let entry = config.providers.openrouter.get_or_insert_with(Default::default);
                                    entry.api_key = store_value;
                                    Ok(format!("OpenRouter API key set ({})", preview))
                                }
                                "polymarket_key" => {
                                    config.tools.polymarket.private_key = Some(store_value);
                                    Ok(format!("Polymarket private key set ({})", preview))
                                }
                                "solana_key" => {
                                    config.tools.solana_private_key = Some(store_value);
                                    Ok(format!("Solana private key set ({})", preview))
                                }
                                "model" => {
                                    config.agents.defaults.model = value.clone();
                                    Ok(format!("Model set to: {}", value))
                                }
                                "max_bet" => {
                                    match value.parse::<f64>() {
                                        Ok(v) => { config.tools.betting.max_bet_size_usdc = v; Ok(format!("Max bet set to ${}", v)) }
                                        Err(_) => Err("Invalid number".to_string())
                                    }
                                }
                                "daily_limit" => {
                                    match value.parse::<f64>() {
                                        Ok(v) => { config.tools.betting.daily_loss_limit_usdc = v; Ok(format!("Daily loss limit set to ${}", v)) }
                                        Err(_) => Err("Invalid number".to_string())
                                    }
                                }
                                _ => Err(format!("Unknown key: `{}`. Use /config to see available keys.", key))
                            };

                            let security_note = if is_secret {
                                "\n🔒 Message auto-deleted · 🔐 Value encrypted (AES-256-GCM)"
                            } else {
                                ""
                            };

                            match result {
                                Ok(success_msg) => {
                                    match config.save() {
                                        Ok(()) => {
                                            let _ = _bot.send_message(msg.chat.id, format!("✅ {} — saved to config.json{}\n⚠️ Restart the bot to apply changes.", success_msg, security_note)).await;
                                        }
                                        Err(e) => {
                                            let _ = _bot.send_message(msg.chat.id, format!("⚠️ {} — but failed to save: {}", success_msg, e)).await;
                                        }
                                    }
                                }
                                Err(err_msg) => {
                                    let _ = _bot.send_message(msg.chat.id, format!("❌ {}", err_msg)).await;
                                }
                            }
                            return respond(());
                        }

                        let _ = _bot.send_message(msg.chat.id, "❌ Unknown config command. Use /config for help.").await;
                        return respond(());
                    }
                    // ── FAST PATH: /polymarket CLI commands (bypass LLM) ──
                    if lower == "polymarket" || lower == "/polymarket"
                        || lower.starts_with("polymarket ") || lower.starts_with("/polymarket ")
                    {
                        // Strip the prefix to get the raw arguments
                        let args_str = if lower == "polymarket" || lower == "/polymarket" {
                            ""
                        } else if normalized.starts_with('/') {
                            normalized[12..].trim() // skip "/polymarket "
                        } else {
                            normalized[11..].trim() // skip "polymarket "
                        };
                        let args_lower = args_str.to_lowercase();

                        // Handle --help / help / bare command
                        if args_str.is_empty() || args_lower == "--help" || args_lower == "help" {
                            use crate::tools::polymarket_help::POLYMARKET_HELP;
                            use crate::gateway::utils::chunk_message;

                            let chunks = chunk_message(POLYMARKET_HELP, TELEGRAM_MAX_LEN);
                            for chunk in chunks {
                                let _ = _bot.send_message(msg.chat.id, chunk).await;
                            }
                            return respond(());
                        }

                        // Parse and execute the CLI command
                        if let Some(parsed_args) = shlex::split(args_str) {
                            use crate::config::Config;
                            let config = Config::load().unwrap_or_default();

                            let progress_msg = format!("⚙️ `polymarket {}`…", parsed_args.join(" "));
                            let _ = _bot.send_message(msg.chat.id, &progress_msg).await;

                            let str_args: Vec<&str> = parsed_args.iter().map(|s| s.as_str()).collect();

                            match crate::tools::polymarket_common::run_polymarket_cli(&config.tools.polymarket, &str_args).await {
                                Ok(output) => {
                                    let content = if output.trim().is_empty() {
                                        "✅ Command completed (no output)".to_string()
                                    } else {
                                        output
                                    };
                                    use crate::gateway::utils::chunk_message;
                                    let chunks = chunk_message(&content, TELEGRAM_MAX_LEN);
                                    for chunk in chunks {
                                        let _ = _bot.send_message(msg.chat.id, chunk).await;
                                    }
                                }
                                Err(e) => {
                                    let err_msg = format!("❌ CLI Error:\n{}", e);
                                    let _ = _bot.send_message(msg.chat.id, err_msg).await;
                                }
                            }
                            return respond(());
                        } else {
                            let _ = _bot.send_message(msg.chat.id, "❌ Could not parse command arguments. Check your quoting.").await;
                            return respond(());
                        }
                    }

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

        let callback_handler = Update::filter_callback_query().endpoint(
            move |bot: Bot, q: CallbackQuery, bus: Arc<MessageBus>, allow_from: Vec<String>| async move {
                let user_id = q.from.id.to_string();

                // Enforce allowFrom ACL
                if !allow_from.is_empty() && !allow_from.contains(&user_id) {
                    warn!(user_id, "Rejected callback query from unauthorized user");
                    return respond(());
                }

                if let (Some(data), Some(msg)) = (q.data, q.message) {
                    info!(user_id, data, "Received callback query");
                    
                    // Treat the button data as an inbound message
                    let inbound = InboundMessage {
                        channel: "telegram".to_owned(),
                        chat_id: msg.chat().id.to_string(),
                        user_id: user_id.clone(),
                        content: data,
                        media: Vec::new(),
                        is_system: false,
                    };

                    if let Err(e) = bus.inbound_sender().send(inbound).await {
                        error!("Failed to send callback inbound to bus: {}", e);
                    }

                    // Acknowledge the callback query to remove the spinner
                    let _ = bot.answer_callback_query(q.id).await;
                }
                respond(())
            },
        );

        let handler = dptree::entry()
            .branch(message_handler)
            .branch(callback_handler);

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

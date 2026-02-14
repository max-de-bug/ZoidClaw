use anyhow::Result;
use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{error, info, warn};
use crate::bus::MessageBus;
use crate::bus::events::InboundMessage;

/// Maximum Telegram message length.
const TELEGRAM_MAX_LEN: usize = 4096;

pub struct TelegramTransport {
    token: String,
    bus: Arc<tokio::sync::Mutex<MessageBus>>,
    allow_from: Vec<String>,
}

impl TelegramTransport {
    pub fn new(
        token: String,
        bus: Arc<tokio::sync::Mutex<MessageBus>>,
        allow_from: Vec<String>,
    ) -> Self {
        Self { token, bus, allow_from }
    }

    pub async fn run(self) -> Result<()> {
        let bot = Bot::new(&self.token);
        
        info!("Telegram transport started");

        // Subscribe to outbound messages FIRST (before dispatcher starts)
        {
            let bot_out = bot.clone();
            let bus_locked = self.bus.lock().await;
            bus_locked.subscribe_outbound("telegram", move |msg| {
                let bot_out = bot_out.clone();
                async move {
                    if let Ok(chat_id) = msg.chat_id.parse::<i64>() {
                        // Chunk long messages to respect Telegram's limit
                        let chunks = chunk_message(&msg.content, TELEGRAM_MAX_LEN);
                        for chunk in chunks {
                            if let Err(e) = bot_out.send_message(ChatId(chat_id), chunk).await {
                                error!("Failed to send Telegram message: {}", e);
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
            move |_bot: Bot, msg: Message, bus: Arc<tokio::sync::Mutex<MessageBus>>, allow_from: Vec<String>| async move {
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

                    let bus_locked = bus.lock().await;
                    if let Err(e) = bus_locked.inbound_sender().send(inbound).await {
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

/// Split a message into chunks of at most `max_len` characters,
/// preferring to break at newlines when possible.
fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_owned()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_owned());
            break;
        }

        // Try to find a newline to break at
        let slice = &remaining[..max_len];
        let break_at = slice.rfind('\n').unwrap_or(max_len);
        let break_at = if break_at == 0 { max_len } else { break_at };

        chunks.push(remaining[..break_at].to_owned());
        remaining = &remaining[break_at..].trim_start_matches('\n');
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_short_message() {
        let chunks = chunk_message("hello", 4096);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_chunk_long_message() {
        let long = "a".repeat(5000);
        let chunks = chunk_message(&long, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    #[test]
    fn test_chunk_at_newline() {
        let text = format!("{}\n{}", "a".repeat(100), "b".repeat(100));
        let chunks = chunk_message(&text, 150);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(100));
        assert_eq!(chunks[1], "b".repeat(100));
    }
}

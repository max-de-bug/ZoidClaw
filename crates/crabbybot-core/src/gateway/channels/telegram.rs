use anyhow::Result;
use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{error, info, warn};
use crate::bus::MessageBus;
use crate::bus::events::InboundMessage;
use crate::gateway::utils::chunk_message;

/// Maximum Telegram message length.
const TELEGRAM_MAX_LEN: usize = 4096;

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

        info!("Telegram transport started");

        // Subscribe to outbound messages FIRST (before dispatcher starts)
        {
            let bot_out = bot.clone();
            self.bus.subscribe_outbound("telegram", move |msg| {
                let bot_out = bot_out.clone();
                async move {
                    if let Ok(chat_id) = msg.chat_id.parse::<i64>() {
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

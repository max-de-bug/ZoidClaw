use anyhow::Result;
use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{error, info};
use crate::bus::MessageBus;
use crate::bus::events::InboundMessage;

pub struct TelegramTransport {
    token: String,
    bus: Arc<tokio::sync::Mutex<MessageBus>>,
}

impl TelegramTransport {
    pub fn new(token: String, bus: Arc<tokio::sync::Mutex<MessageBus>>) -> Self {
        Self { token, bus }
    }

    pub async fn run(self) -> Result<()> {
        let bot = Bot::new(&self.token);
        
        info!("Telegram transport started");

        // Set up inbound update handler
        let bus = Arc::clone(&self.bus);
        let handler = Update::filter_message().endpoint(
            move |_bot: Bot, msg: Message, bus: Arc<tokio::sync::Mutex<MessageBus>>| async move {
                if let Some(text) = msg.text() {
                    let inbound = InboundMessage {
                        channel: "telegram".to_owned(),
                        chat_id: msg.chat.id.to_string(),
                        user_id: msg.from.as_ref().map(|u| u.id.to_string()).unwrap_or_else(|| "unknown".to_owned()),
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

        // Subscribe to outbound messages
        {
            let bot = bot.clone();
            let mut bus_locked = self.bus.lock().await;
            bus_locked.subscribe_outbound("telegram", move |msg| {
                let bot = bot.clone();
                async move {
                    if let Ok(chat_id) = msg.chat_id.parse::<i64>() {
                        if let Err(e) = bot.send_message(ChatId(chat_id), msg.content).await {
                            error!("Failed to send Telegram message: {}", e);
                        }
                    }
                }
            });
        }

        Dispatcher::builder(bot, handler)
            .dependencies(dptree::deps![bus])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;

        Ok(())
    }
}

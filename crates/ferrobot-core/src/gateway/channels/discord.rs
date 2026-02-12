use anyhow::Result;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::sync::Arc;
use tracing::{error, info};
use crate::bus::MessageBus;
use crate::bus::events::InboundMessage;

struct Handler {
    bus: Arc<tokio::sync::Mutex<MessageBus>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let inbound = InboundMessage {
            channel: "discord".to_owned(),
            chat_id: msg.channel_id.to_string(),
            user_id: msg.author.id.to_string(),
            content: msg.content.clone(),
            media: Vec::new(),
            is_system: false,
        };

        let bus_locked = self.bus.lock().await;
        if let Err(e) = bus_locked.inbound_sender().send(inbound).await {
            error!("Failed to send inbound message to bus: {}", e);
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        info!("Discord transport ready: {}", ready.user.name);
    }
}

pub struct DiscordTransport {
    token: String,
    bus: Arc<tokio::sync::Mutex<MessageBus>>,
}

impl DiscordTransport {
    pub fn new(token: String, bus: Arc<tokio::sync::Mutex<MessageBus>>) -> Self {
        Self { token, bus }
    }

    pub async fn run(self) -> Result<()> {
        let mut client = Client::builder(&self.token, GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT)
            .event_handler(Handler { bus: Arc::clone(&self.bus) })
            .await?;

        // Subscribe to outbound messages
        {
            let http = Arc::clone(&client.http);
            let mut bus_locked = self.bus.lock().await;
            bus_locked.subscribe_outbound("discord", move |msg| {
                let http = Arc::clone(&http);
                async move {
                    if let Ok(channel_id) = msg.chat_id.parse::<u64>() {
                        use serenity::model::id::ChannelId;
                        if let Err(e) = ChannelId::new(channel_id).say(&http, msg.content).await {
                            error!("Failed to send Discord message: {}", e);
                        }
                    }
                }
            });
        }

        info!("Discord transport starting...");
        client.start().await?;

        Ok(())
    }
}

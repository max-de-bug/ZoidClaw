use anyhow::Result;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::sync::Arc;
use tracing::{error, info, warn};
use crate::bus::MessageBus;
use crate::bus::events::InboundMessage;

struct Handler {
    bus: Arc<tokio::sync::Mutex<MessageBus>>,
    allow_from: Vec<String>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let user_id = msg.author.id.to_string();

        // Enforce allowFrom ACL
        if !self.allow_from.is_empty() && !self.allow_from.contains(&user_id) {
            warn!(
                user_id = user_id,
                channel_id = msg.channel_id.to_string(),
                "Rejected Discord message from user not in allowFrom list"
            );
            return;
        }

        let inbound = InboundMessage {
            channel: "discord".to_owned(),
            chat_id: msg.channel_id.to_string(),
            user_id,
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
    allow_from: Vec<String>,
}

impl DiscordTransport {
    pub fn new(
        token: String,
        bus: Arc<tokio::sync::Mutex<MessageBus>>,
        allow_from: Vec<String>,
    ) -> Self {
        Self { token, bus, allow_from }
    }

    pub async fn run(self) -> Result<()> {
        let mut client = Client::builder(&self.token, GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT | GatewayIntents::DIRECT_MESSAGES)
            .event_handler(Handler {
                bus: Arc::clone(&self.bus),
                allow_from: self.allow_from,
            })
            .await?;

        // Subscribe to outbound messages
        {
            let http = Arc::clone(&client.http);
            let bus_locked = self.bus.lock().await;
            bus_locked.subscribe_outbound("discord", move |msg| {
                let http = Arc::clone(&http);
                async move {
                    if let Ok(channel_id) = msg.chat_id.parse::<u64>() {
                        use serenity::model::id::ChannelId;
                        // Discord has a 2000-char limit
                        let chunks = chunk_discord_message(&msg.content, 2000);
                        for chunk in chunks {
                            if let Err(e) = ChannelId::new(channel_id).say(&http, chunk).await {
                                error!("Failed to send Discord message: {}", e);
                            }
                        }
                    }
                }
            }).await;
        }

        info!("Discord transport starting...");
        client.start().await?;

        Ok(())
    }
}

/// Split a message into chunks of at most `max_len` characters.
fn chunk_discord_message(text: &str, max_len: usize) -> Vec<String> {
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

        let slice = &remaining[..max_len];
        let break_at = slice.rfind('\n').unwrap_or(max_len);
        let break_at = if break_at == 0 { max_len } else { break_at };

        chunks.push(remaining[..break_at].to_owned());
        remaining = &remaining[break_at..].trim_start_matches('\n');
    }

    chunks
}

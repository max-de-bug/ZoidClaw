use crate::bus::events::{InboundMessage, OutboundMessage};
use crate::bus::MessageBus;
use crate::gateway::utils::chunk_message;
use anyhow::Result;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Maximum Discord message length.
const DISCORD_MAX_LEN: usize = 2000;

struct Handler {
    bus: Arc<MessageBus>,
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

        if let Err(e) = self.bus.inbound_sender().send(inbound).await {
            error!("Failed to send inbound message to bus: {}", e);
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        info!("Discord transport ready: {}", ready.user.name);
    }
}

pub struct DiscordTransport {
    token: String,
    bus: Arc<MessageBus>,
    allow_from: Vec<String>,
}

impl DiscordTransport {
    pub fn new(token: String, bus: Arc<MessageBus>, allow_from: Vec<String>) -> Self {
        Self {
            token,
            bus,
            allow_from,
        }
    }

    pub async fn run(self) -> Result<()> {
        let mut client = Client::builder(
            &self.token,
            GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::MESSAGE_CONTENT
                | GatewayIntents::DIRECT_MESSAGES,
        )
        .event_handler(Handler {
            bus: Arc::clone(&self.bus),
            allow_from: self.allow_from,
        })
        .await?;

        // Subscribe to outbound messages
        {
            let http = Arc::clone(&client.http);
            self.bus
                .subscribe_outbound("discord", move |msg| {
                    let http = Arc::clone(&http);
                    async move {
                        match msg {
                            OutboundMessage::Reply {
                                chat_id, content, ..
                            }
                            | OutboundMessage::Progress {
                                chat_id, content, ..
                            } => {
                                if let Ok(channel_id) = chat_id.parse::<u64>() {
                                    let chunks = chunk_message(&content, DISCORD_MAX_LEN);
                                    for chunk in chunks {
                                        if let Err(e) =
                                            ChannelId::new(channel_id).say(&http, chunk).await
                                        {
                                            error!("Failed to send Discord message: {}", e);
                                        }
                                    }
                                }
                            }
                            // Discord doesn't expose a simple typing indicator via this API path
                            OutboundMessage::Typing { .. } => {}
                        }
                    }
                })
                .await;
        }

        info!("Discord transport starting...");
        client.start().await?;

        Ok(())
    }
}

//! Message bus event types.
//!
//! Defines the messages that flow between channels and the agent core.

/// An inbound message from a chat channel to the agent.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// Source channel identifier (e.g., "telegram", "cli").
    pub channel: String,
    /// Chat/conversation identifier within the channel.
    pub chat_id: String,
    /// User identifier.
    pub user_id: String,
    /// Message text content.
    pub content: String,
    /// Optional media attachment paths (images, voice, etc.).
    pub media: Vec<String>,
    /// Whether this is a system-originated message (e.g., subagent result).
    pub is_system: bool,
}

/// An outbound message from the agent to a chat channel.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    /// Destination channel identifier.
    pub channel: String,
    /// Destination chat/conversation identifier.
    pub chat_id: String,
    /// Response text content.
    pub content: String,
}

impl InboundMessage {
    /// Create a simple CLI inbound message.
    pub fn cli(content: &str) -> Self {
        Self {
            channel: "cli".into(),
            chat_id: "direct".into(),
            user_id: "user".into(),
            content: content.into(),
            media: Vec::new(),
            is_system: false,
        }
    }
}

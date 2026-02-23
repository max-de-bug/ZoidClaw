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
///
/// Channels should handle all variants:
/// - `Reply`    — final text response, always rendered.
/// - `Typing`   — show a "typing…" indicator (best-effort, ignore if unsupported).
/// - `Progress` — intermediate status line shown while tools are executing.
#[derive(Debug, Clone)]
pub enum OutboundMessage {
    /// Final text reply from the agent.
    Reply {
        channel: String,
        chat_id: String,
        content: String,
        buttons: Option<Vec<Button>>,
    },
    /// Ask the channel to display a "typing…" indicator.
    Typing {
        channel: String,
        chat_id: String,
    },
    /// Intermediate progress update (e.g., "Running tool: read_file…").
    Progress {
        channel: String,
        chat_id: String,
        content: String,
    },
}

/// Actions for controlling background services.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamAction {
    Start,
    Stop,
}

/// Internal coordination messages between components.
#[derive(Debug, Clone)]
pub enum InternalMessage {
    /// Control the Pump.fun real-time stream.
    StreamControl {
        action: StreamAction,
        chat_id: String,
    },
}

/// A UI button that can be attached to a message.
#[derive(Debug, Clone)]
pub struct Button {
    pub text: String,
    pub data: Option<String>,
    pub url: Option<String>,
}

impl OutboundMessage {
    /// Convenience: create a `Reply` message without buttons.
    pub fn reply(channel: impl Into<String>, chat_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Reply {
            channel: channel.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            buttons: None,
        }
    }

    /// Convenience: create a `Reply` message with buttons.
    pub fn reply_with_buttons(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
        buttons: Vec<Button>,
    ) -> Self {
        Self::Reply {
            channel: channel.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            buttons: Some(buttons),
        }
    }

    /// Convenience: create a `Typing` message.
    pub fn typing(channel: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self::Typing {
            channel: channel.into(),
            chat_id: chat_id.into(),
        }
    }

    /// Convenience: create a `Progress` message.
    pub fn progress(channel: impl Into<String>, chat_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Progress {
            channel: channel.into(),
            chat_id: chat_id.into(),
            content: content.into(),
        }
    }

    /// Extract the channel name regardless of variant.
    pub fn channel(&self) -> &str {
        match self {
            Self::Reply { channel, .. } => channel,
            Self::Typing { channel, .. } => channel,
            Self::Progress { channel, .. } => channel,
        }
    }

    /// Extract the chat_id regardless of variant.
    pub fn chat_id(&self) -> &str {
        match self {
            Self::Reply { chat_id, .. } => chat_id,
            Self::Typing { chat_id, .. } => chat_id,
            Self::Progress { chat_id, .. } => chat_id,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reply_variant() {
        let msg = OutboundMessage::reply("telegram", "chat123", "Hello!");
        assert_eq!(msg.channel(), "telegram");
        assert_eq!(msg.chat_id(), "chat123");
        match msg {
            OutboundMessage::Reply { buttons, .. } => assert!(buttons.is_none()),
            _ => panic!("Expected Reply variant"),
        }
    }

    #[test]
    fn test_typing_variant() {
        let msg = OutboundMessage::typing("telegram", "chat123");
        assert_eq!(msg.channel(), "telegram");
        assert!(matches!(msg, OutboundMessage::Typing { .. }));
    }

    #[test]
    fn test_progress_variant() {
        let msg = OutboundMessage::progress("cli", "direct", "Running tool: read_file…");
        assert!(matches!(msg, OutboundMessage::Progress { .. }));
    }
}

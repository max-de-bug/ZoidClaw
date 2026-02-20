//! Heartbeat module for proactive agent wake-up.
//!
//! Mirrors nanobot's `heartbeat/` module. A `Heartbeat` instance wakes up
//! at a fixed interval and pushes a **system** `InboundMessage` to the bus
//! so the agent can perform autonomous actions (e.g., daily summaries,
//! portfolio price checks, scheduled reminders).
//!
//! Unlike the `cron` module (which is user-scheduled and persisted to disk),
//! heartbeats are ephemeral, in-process triggers that are recreated on every
//! bot start.
//!
//! # Example
//!
//! ```no_run
//! use std::time::Duration;
//! use tokio_util::sync::CancellationToken;
//! use crabbybot_core::heartbeat::Heartbeat;
//! use crabbybot_core::bus::events::InboundMessage;
//!
//! # async fn example(tx: tokio::sync::mpsc::Sender<InboundMessage>) {
//! let cancel = CancellationToken::new();
//! let hb = Heartbeat::builder()
//!     .interval(Duration::from_secs(3600))
//!     .message("Heartbeat: perform your daily summary.")
//!     .channel("cli")
//!     .chat_id("direct")
//!     .build();
//!
//! tokio::spawn(hb.run(tx, cancel.clone()));
//! # }
//! ```

use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::bus::events::InboundMessage;

/// A proactive wake-up trigger.
///
/// Every `interval` seconds, a system `InboundMessage` is sent to the bus
/// inbound channel so the agent can act autonomously.
pub struct Heartbeat {
    interval: Duration,
    message: String,
    channel: String,
    chat_id: String,
}

impl Heartbeat {
    /// Start building a heartbeat with [`HeartbeatBuilder`].
    pub fn builder() -> HeartbeatBuilder {
        HeartbeatBuilder::default()
    }

    /// Run the heartbeat loop until `cancel` is triggered or the sender closes.
    ///
    /// The first beat fires *after* the configured interval (not immediately),
    /// so the agent has time to start up before receiving its first prompt.
    pub async fn run(self, tx: mpsc::Sender<InboundMessage>, cancel: CancellationToken) {
        info!(
            interval_secs = self.interval.as_secs(),
            channel = self.channel,
            "Heartbeat started"
        );

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Heartbeat cancelled");
                    return;
                }
                _ = tokio::time::sleep(self.interval) => {
                    let msg = InboundMessage {
                        channel: self.channel.clone(),
                        chat_id: self.chat_id.clone(),
                        user_id: "heartbeat".into(),
                        content: self.message.clone(),
                        media: Vec::new(),
                        is_system: true,
                    };

                    info!(channel = self.channel, "Heartbeat firing");

                    if tx.send(msg).await.is_err() {
                        // Bus shut down — stop the heartbeat.
                        return;
                    }
                }
            }
        }
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for [`Heartbeat`].
#[derive(Default)]
pub struct HeartbeatBuilder {
    interval: Option<Duration>,
    message: Option<String>,
    channel: Option<String>,
    chat_id: Option<String>,
}

impl HeartbeatBuilder {
    /// Set the interval between beats (required).
    pub fn interval(mut self, d: Duration) -> Self {
        self.interval = Some(d);
        self
    }

    /// Set the message sent to the agent on each beat (required).
    pub fn message(mut self, m: impl Into<String>) -> Self {
        self.message = Some(m.into());
        self
    }

    /// Set the target channel (defaults to `"cli"`).
    pub fn channel(mut self, c: impl Into<String>) -> Self {
        self.channel = Some(c.into());
        self
    }

    /// Set the target chat ID (defaults to `"direct"`).
    pub fn chat_id(mut self, id: impl Into<String>) -> Self {
        self.chat_id = Some(id.into());
        self
    }

    /// Build the [`Heartbeat`].
    ///
    /// # Panics
    /// Panics if `interval` or `message` were not set.
    pub fn build(self) -> Heartbeat {
        Heartbeat {
            interval: self.interval.expect("Heartbeat::builder: interval is required"),
            message: self.message.expect("Heartbeat::builder: message is required"),
            channel: self.channel.unwrap_or_else(|| "cli".into()),
            chat_id: self.chat_id.unwrap_or_else(|| "direct".into()),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that a heartbeat fires at least once within 2× its interval.
    #[tokio::test]
    async fn test_heartbeat_fires() {
        let (tx, mut rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();

        let hb = Heartbeat::builder()
            .interval(Duration::from_millis(50))
            .message("ping")
            .channel("cli")
            .chat_id("test")
            .build();

        let cancel_clone = cancel.clone();
        tokio::spawn(hb.run(tx, cancel_clone));

        // Wait up to 200 ms for the first beat.
        let msg = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("heartbeat did not fire within 200 ms")
            .expect("channel closed");

        assert_eq!(msg.content, "ping");
        assert!(msg.is_system, "heartbeat messages must be marked as system");
        assert_eq!(msg.channel, "cli");

        cancel.cancel();
    }

    /// Verify that cancelling stops the heartbeat.
    #[tokio::test]
    async fn test_heartbeat_cancels() {
        let (tx, mut rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();

        let hb = Heartbeat::builder()
            .interval(Duration::from_secs(3600)) // very long — should never fire
            .message("should not appear")
            .build();

        let cancel_clone = cancel.clone();
        tokio::spawn(hb.run(tx, cancel_clone));

        // Cancel immediately
        cancel.cancel();

        // Give the task a moment to exit
        tokio::time::sleep(Duration::from_millis(20)).await;

        // No messages should have been sent
        assert!(rx.try_recv().is_err(), "no heartbeat should have fired after cancel");
    }
}

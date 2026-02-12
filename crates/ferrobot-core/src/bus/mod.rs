//! Async message bus for decoupled channel-agent communication.
//!
//! Uses `tokio::sync::mpsc` channels instead of Python's `asyncio.Queue`.
//! This gives us true multi-producer, single-consumer semantics with
//! proper backpressure.

pub mod events;

use events::{InboundMessage, OutboundMessage};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Callback type for outbound message subscribers.
type OutboundCallback =
    Box<dyn Fn(OutboundMessage) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

/// Async message bus that decouples chat channels from the agent core.
///
/// Channels push messages to the inbound sender, and the agent processes
/// them via the inbound receiver. Responses flow back through the
/// outbound channel to registered subscribers.
pub struct MessageBus {
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
    subscribers: HashMap<String, Vec<OutboundCallback>>,
}

pub struct MessageBusReceivers {
    pub inbound_rx: mpsc::Receiver<InboundMessage>,
    pub outbound_rx: mpsc::Receiver<OutboundMessage>,
}

impl MessageBus {
    /// Create a new message bus with the given channel capacity.
    pub fn new(capacity: usize) -> (Self, MessageBusReceivers) {
        let (inbound_tx, inbound_rx) = mpsc::channel(capacity);
        let (outbound_tx, outbound_rx) = mpsc::channel(capacity);

        (
            Self {
                inbound_tx,
                outbound_tx,
                subscribers: HashMap::new(),
            },
            MessageBusReceivers {
                inbound_rx,
                outbound_rx,
            },
        )
    }

    /// Get a cloneable sender for publishing inbound messages.
    pub fn inbound_sender(&self) -> mpsc::Sender<InboundMessage> {
        self.inbound_tx.clone()
    }

    /// Publish an outbound message.
    pub async fn publish_outbound(&self, msg: OutboundMessage) {
        if let Err(e) = self.outbound_tx.send(msg).await {
            error!("Failed to publish outbound message: {}", e);
        }
    }

    /// Subscribe to outbound messages for a specific channel.
    pub fn subscribe_outbound<F, Fut>(&mut self, channel: &str, callback: F)
    where
        F: Fn(OutboundMessage) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let boxed: OutboundCallback = Box::new(move |msg| Box::pin(callback(msg)));
        self.subscribers
            .entry(channel.to_string())
            .or_default()
            .push(boxed);
    }

    /// Dispatch outbound messages to subscribers.
    /// Run this as a background task via `tokio::spawn`.
    pub async fn dispatch_outbound(&mut self, mut outbound_rx: mpsc::Receiver<OutboundMessage>) {
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(callbacks) = self.subscribers.get(&msg.channel) {
                for callback in callbacks {
                    let fut = callback(msg.clone());
                    if let Err(e) =
                        tokio::time::timeout(std::time::Duration::from_secs(10), fut).await
                    {
                        error!(channel = msg.channel, "Outbound dispatch timed out: {}", e);
                    }
                }
            } else {
                debug!(channel = msg.channel, "No subscribers for outbound message");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inbound_send_receive() {
        let (bus, mut receivers) = MessageBus::new(16);
        let tx = bus.inbound_sender();

        tx.send(InboundMessage::cli("hello")).await.unwrap();

        let msg = receivers.inbound_rx.recv().await.unwrap();
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.channel, "cli");
    }
}

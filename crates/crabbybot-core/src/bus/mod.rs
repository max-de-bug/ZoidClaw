//! Async message bus for decoupled channel-agent communication.
//!
//! Uses `tokio::sync::mpsc` channels instead of Python's `asyncio.Queue`.
//! This gives us true multi-producer, single-consumer semantics with
//! proper backpressure.
//!
//! Subscribers are stored in a shared `Arc<RwLock>` map so the outbound
//! dispatch loop can run without holding the bus mutex.

pub mod events;

use events::{InboundMessage, OutboundMessage};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error};

/// Callback type for outbound message subscribers.
type OutboundCallback =
    Box<dyn Fn(OutboundMessage) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

/// Shared subscriber map — can be cloned and read without locking the bus.
pub type SubscriberMap = Arc<RwLock<HashMap<String, Vec<OutboundCallback>>>>;

/// Async message bus that decouples chat channels from the agent core.
///
/// Channels push messages to the inbound sender, and the agent processes
/// them via the inbound receiver. Responses flow back through the
/// outbound channel to registered subscribers.
pub struct MessageBus {
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
    subscribers: SubscriberMap,
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
                subscribers: Arc::new(RwLock::new(HashMap::new())),
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

    /// Get a clone of the subscriber map for use in dispatch or registration.
    pub fn subscribers(&self) -> SubscriberMap {
        Arc::clone(&self.subscribers)
    }

    /// Subscribe to outbound messages for a specific channel.
    ///
    /// The callback receives *all* `OutboundMessage` variants for the channel;
    /// implementations should match on the variant and ignore unknowns.
    ///
    /// This takes `&self` (not `&mut self`) — safe to call from any task
    /// because the subscriber map uses an internal `RwLock`.
    pub async fn subscribe_outbound<F, Fut>(&self, channel: &str, callback: F)
    where
        F: Fn(OutboundMessage) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let boxed: OutboundCallback = Box::new(move |msg| Box::pin(callback(msg)));
        let mut subs = self.subscribers.write().await;
        subs.entry(channel.to_string())
            .or_default()
            .push(boxed);
    }
}

/// Dispatch outbound messages to subscribers.
///
/// Routes each `OutboundMessage` to all callbacks registered for
/// `msg.channel()`. Callbacks receive the full enum variant so they can
/// handle typing indicators, progress updates, and final replies differently.
///
/// This is a **free function** — it does not hold the bus mutex, only the
/// shared subscriber map. Run it as a background task via `tokio::spawn`.
pub async fn dispatch_outbound(
    subscribers: SubscriberMap,
    mut outbound_rx: mpsc::Receiver<OutboundMessage>,
) {
    while let Some(msg) = outbound_rx.recv().await {
        let channel = msg.channel().to_owned();
        let subs = subscribers.read().await;
        if let Some(callbacks) = subs.get(&channel) {
            for callback in callbacks {
                let fut = callback(msg.clone());
                if let Err(e) =
                    tokio::time::timeout(std::time::Duration::from_secs(10), fut).await
                {
                    error!(channel = %channel, "Outbound dispatch timed out: {}", e);
                }
            }
        } else {
            debug!(channel = %channel, "No subscribers for outbound message");
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

    #[tokio::test]
    async fn test_outbound_dispatch_to_subscriber() {
        let (bus, receivers) = MessageBus::new(16);

        // Register a subscriber that captures reply messages
        let received = Arc::new(RwLock::new(Vec::<String>::new()));
        let received_clone = Arc::clone(&received);

        bus.subscribe_outbound("test_channel", move |msg| {
            let captured = Arc::clone(&received_clone);
            async move {
                if let OutboundMessage::Reply { content, .. } = msg {
                    captured.write().await.push(content);
                }
            }
        }).await;

        // Get the subscribers map and start dispatch in background
        let subs = bus.subscribers();
        let dispatch_handle = tokio::spawn(dispatch_outbound(subs, receivers.outbound_rx));

        // Publish a Reply message
        bus.publish_outbound(OutboundMessage::reply(
            "test_channel",
            "chat1",
            "hello subscriber",
        )).await;

        // Give dispatch a moment to process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify
        let msgs = received.read().await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], "hello subscriber");

        // Clean up
        drop(bus); // drops outbound_tx, causing dispatch to exit
        let _ = dispatch_handle.await;
    }

    #[tokio::test]
    async fn test_subscribe_after_creation() {
        let (bus, receivers) = MessageBus::new(16);
        let subs = bus.subscribers();

        // Start dispatch BEFORE subscribing (the original race condition)
        let dispatch_handle = tokio::spawn(dispatch_outbound(
            Arc::clone(&subs),
            receivers.outbound_rx,
        ));

        // Subscribe AFTER dispatch starts — this should still work
        let received = Arc::new(RwLock::new(false));
        let received_clone = Arc::clone(&received);
        bus.subscribe_outbound("late_channel", move |_msg| {
            let flag = Arc::clone(&received_clone);
            async move {
                *flag.write().await = true;
            }
        }).await;

        bus.publish_outbound(OutboundMessage::reply(
            "late_channel",
            "c1",
            "late message",
        )).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(*received.read().await);

        drop(bus);
        let _ = dispatch_handle.await;
    }
}

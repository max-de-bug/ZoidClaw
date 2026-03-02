use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::bus::events::{Button, InternalMessage, OutboundMessage, StreamAction};
use crate::bus::MessageBus;
use crate::config::PumpFunStreamConfig;

pub struct PumpFunStream {
    bus: Arc<MessageBus>,
    config: PumpFunStreamConfig,
    state: Arc<Mutex<StreamState>>,
}

pub struct StreamState {
    pub worker: Option<(JoinHandle<()>, CancellationToken)>,
    pub active_chat_id: Option<String>,
}

impl PumpFunStream {
    pub fn new(bus: Arc<MessageBus>, config: PumpFunStreamConfig) -> Self {
        Self {
            bus,
            config,
            state: Arc::new(Mutex::new(StreamState {
                worker: None,
                active_chat_id: None,
            })),
        }
    }

    /// Get a handle to the shared stream state.
    pub fn state(&self) -> Arc<Mutex<StreamState>> {
        Arc::clone(&self.state)
    }

    /// Run the control loop for the stream service.
    ///
    /// Listens for internal control messages to start/stop the WebSocket worker.
    pub async fn run(&self, mut internal_rx: mpsc::Receiver<InternalMessage>) {
        info!("Pump.fun Stream Service started and waiting for control signals");

        // If enabled by default in config, we could start it here.
        // But the user requested it NOT to start automatically.
        if self.config.enabled && !self.config.chat_id.is_empty() {
            info!("Pump.fun streaming is configured but waiting for manual activation as per user request.");
        }

        while let Some(msg) = internal_rx.recv().await {
            match msg {
                InternalMessage::StreamControl { action, chat_id } => match action {
                    StreamAction::Start => self.start(chat_id).await,
                    StreamAction::Stop => self.stop().await,
                },
            }
        }
    }

    async fn start(&self, chat_id: String) {
        let mut state = self.state.lock().await;

        // Stop any existing worker
        if let Some((handle, token)) = state.worker.take() {
            token.cancel();
            let _ = handle.await;
        }

        info!("Starting Pump.fun real-time stream for chat: {}", chat_id);
        state.active_chat_id = Some(chat_id.clone());

        let token = CancellationToken::new();
        let bus = Arc::clone(&self.bus);
        let worker_token = token.clone();
        let worker_chat_id = chat_id.clone();

        let handle = tokio::spawn(async move {
            let url = "wss://pumpportal.fun/api/data";

            loop {
                tokio::select! {
                    _ = worker_token.cancelled() => {
                        info!("Pump.fun stream worker stopped by signal");
                        return;
                    }
                    conn_result = connect_async(url) => {
                        match conn_result {
                            Ok((mut ws_stream, _)) => {
                                info!("Connected to PumpPortal");

                                let subscribe_msg = json!({ "method": "subscribeNewToken" });
                                info!("Sending subscription request to PumpPortal: {}", subscribe_msg);
                                let text_bytes = tokio_tungstenite::tungstenite::Utf8Bytes::from(subscribe_msg.to_string());

                                if let Err(e) = ws_stream.send(Message::Text(text_bytes)).await {
                                    error!("Failed to send subscription: {}", e);
                                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                                    continue;
                                }
                                info!("Subscription request sent successfully.");

                                while let Some(msg) = ws_stream.next().await {
                                    tokio::select! {
                                        _ = worker_token.cancelled() => return,
                                        msg_val = futures::future::ready(msg) => {
                                            match msg_val {
                                                Ok(Message::Text(text)) => {
                                                    let text_str = text.to_string();
                                                    tracing::debug!("Received WebSocket message: {}", text_str);
                                                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text_str) {
                                                        if event["mint"].is_string() {
                                                            info!("New token detected: {} ({})", event["name"].as_str().unwrap_or("?"), event["mint"].as_str().unwrap_or("?"));
                                                            handle_token_event(&bus, &worker_chat_id, event).await;
                                                        } else {
                                                            tracing::debug!("Received non-token message or heartbeat: {}", text_str);
                                                        }
                                                    }
                                                }
                                                Ok(Message::Close(_)) => break,
                                                Err(e) => {
                                                    error!("WebSocket error: {}", e);
                                                    break;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to connect to PumpPortal: {}. Retrying in 5s...", e);
                            }
                        }
                    }
                }

                // Wait before reconnecting, but be interruptible
                tokio::select! {
                     _ = worker_token.cancelled() => return,
                     _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {}
                }
            }
        });

        state.worker = Some((handle, token));
    }

    async fn stop(&self) {
        let mut state = self.state.lock().await;
        if let Some((handle, token)) = state.worker.take() {
            info!("Stopping Pump.fun real-time stream");
            token.cancel();
            let _ = handle.await;
        }
        state.active_chat_id = None;
    }
}

async fn handle_token_event(bus: &Arc<MessageBus>, chat_id: &str, event: serde_json::Value) {
    let name = event["name"].as_str().unwrap_or("Unknown");
    let symbol = event["symbol"].as_str().unwrap_or("?");
    let mint = event["mint"].as_str().unwrap_or("");

    if mint.is_empty() {
        return;
    }

    let content = format!(
        "🚀 **New Pump.fun Token Launch!**\n\n\
         Token: **{}** ({})\n\
         Mint: `{}`\n\n\
         _Scan the token or buy instantly below:_ ",
        name, symbol, mint
    );

    let buttons = vec![
        Button {
            text: "⚡ Quick Buy (0.1 SOL)".to_string(),
            data: Some(format!("/buy {} 0.1", mint)),
            url: None,
        },
        Button {
            text: "🛡️ Alpha Score".to_string(),
            data: Some(format!("/alpha {}", mint)),
            url: None,
        },
        Button {
            text: "📊 Chart".to_string(),
            data: None,
            url: Some(format!("https://dexscreener.com/solana/{}", mint)),
        },
    ];

    let msg = OutboundMessage::Reply {
        channel: "telegram".into(),
        chat_id: chat_id.to_string(),
        content,
        buttons: Some(buttons),
    };

    bus.publish_outbound(msg).await;
}

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
    println!("Connecting to {}...", url);
    
    let (ws_stream, _) = connect_async(url).await.unwrap();
    let (mut sink, mut stream) = ws_stream.split();
    
    let sub_req = serde_json::json!({
        "type": "market",
        "operation": "subscribe",
        "assets_ids": ["75467129615908319583031474642658885479135630431889036121812713428992454630178"],
        "initial_dump": true
    });
    
    let payload = serde_json::to_string(&sub_req).unwrap();
    println!("Sending subscription: {}", payload);
    sink.send(Message::Text(payload.into())).await.unwrap();
    
    println!("Waiting for messages...");
    for _ in 0..5 {
        if let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Text(t)) => println!("RECEIVED TEXT: {}", t),
                Ok(Message::Binary(b)) => println!("RECEIVED BINARY: {}", String::from_utf8_lossy(&b)),
                Ok(other) => println!("RECEIVED OTHER: {:?}", other),
                Err(e) => println!("ERROR: {}", e),
            }
        }
    }
}

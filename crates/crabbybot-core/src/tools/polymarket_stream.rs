//! Polymarket real-time WebSocket streaming tool.
//!
//! Connects **natively** to the Polymarket CLOB WebSocket API using
//! `tokio-tungstenite` — no subprocess shelling, no SDK dependency.
//!
//! The tool subscribes to the requested asset IDs, collects a bounded
//! number of events (with a timeout), and returns them as structured text
//! that the LLM agent can consume directly.

use std::str;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use async_trait::async_trait;
use futures::{SinkExt as _, StreamExt as _};
use rustls::crypto::ring::default_provider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;
use alloy::primitives::U256;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use super::Tool;

// ── Constants ──────────────────────────────────────────────────────

const WS_MARKET_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

/// Maximum wall-clock time we spend waiting for events before returning
/// whatever we've collected so far.
const STREAM_TIMEOUT: Duration = Duration::from_secs(15);

/// Default number of events to collect if the caller doesn't specify.
const DEFAULT_MAX_EVENTS: u64 = 5;

// ── WebSocket Protocol Types ──────────────────────────────────────

#[derive(Serialize)]
struct SubscribeRequest<'a> {
    r#type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<&'static str>,
    #[serde(rename = "assets_ids")]
    asset_ids: &'a [String],
    markets: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_dump: Option<bool>,
}

impl<'a> SubscribeRequest<'a> {
    fn market(asset_ids: &'a [String]) -> Self {
        Self {
            r#type: "market",
            operation: Some("subscribe"),
            asset_ids,
            markets: &[],
            initial_dump: Some(true),
        }
    }
}

/// Loosely-typed inbound event — survives upstream schema changes.
#[derive(Debug, Clone, Deserialize)]
struct WsEvent {
    event_type: String,
    #[serde(flatten)]
    payload: Value,
}

// ── Tool ──────────────────────────────────────────────────────────

/// Stream real-time Polymarket WebSocket data and return collected events.
#[derive(Clone)]
pub struct PolymarketStreamTool;

impl PolymarketStreamTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketStreamTool {
    fn name(&self) -> &str {
        "polymarket_stream"
    }

    fn description(&self) -> &str {
        "Stream real-time Polymarket market data via WebSocket. \
         Subscribes to the given token/asset IDs and collects up to \
         `max_events` matching events of the specified type. \
         Supported event types: orderbook, prices, last_trade, midpoints. \
         Returns events as formatted text. No wallet needed."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_ids": {
                    "type": "string",
                    "description": "Comma-separated token/asset IDs to subscribe to"
                },
                "event_type": {
                    "type": "string",
                    "enum": ["orderbook", "prices", "last_trade", "midpoints"],
                    "description": "Type of events to stream: orderbook (book snapshots), prices (price changes), last_trade (last trade prices), midpoints (calculated midpoints)"
                },
                "max_events": {
                    "type": "integer",
                    "description": "Maximum number of events to collect before returning (default: 5, max: 20)"
                }
            },
            "required": ["token_ids", "event_type"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        match self.run(args).await {
            Ok(output) => output,
            Err(e) => format!("❌ WebSocket stream error: {e}"),
        }
    }
}

impl PolymarketStreamTool {
    async fn run(&self, args: HashMap<String, Value>) -> Result<String> {
        // ── Parse arguments ────────────────────────────────────────
        let token_ids_raw = args
            .get("token_ids")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Missing required parameter: token_ids"))?;

        let mut asset_ids: Vec<String> = token_ids_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Polymarket WS API strictly requires token IDs in decimal string format,
        // but users/UI often provide them in hex (0x...). Convert them here.
        for id in &mut asset_ids {
            if let Ok(u) = U256::from_str(id) {
                *id = u.to_string();
            }
        }

        if asset_ids.is_empty() {
            return Err(anyhow!("At least one token ID is required"));
        }

        let event_type_arg = args
            .get("event_type")
            .and_then(Value::as_str)
            .unwrap_or("orderbook");

        let ws_filter = match event_type_arg {
            "orderbook" => "book",
            "prices" => "price_change",
            "last_trade" => "last_trade_price",
            "midpoints" => "midpoint",
            other => return Err(anyhow!("Unknown event_type: {other}")),
        };

        let max_events = args
            .get("max_events")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_MAX_EVENTS)
            .min(20);

        // ── Connect & subscribe ────────────────────────────────────
        let _ = default_provider().install_default();

        let (ws_stream, _response) = connect_async(WS_MARKET_URL)
            .await
            .context("Failed to connect to Polymarket WebSocket")?;

        let (mut sink, stream) = ws_stream.split();

        let subscribe = SubscribeRequest::market(&asset_ids);
        let payload = serde_json::to_string(&subscribe)?;
        sink.send(Message::Text(payload.into())).await?;

        // ── Collect events with timeout ────────────────────────────
        let mut events: Vec<WsEvent> = Vec::with_capacity(max_events as usize);
        let deadline = tokio::time::Instant::now() + STREAM_TIMEOUT;

        // Flatten frames → events
        let mut event_stream = stream.flat_map(|frame_result| {
            let parsed: Vec<Result<WsEvent>> = match frame_result {
                Err(e) => vec![Err(anyhow!("WebSocket error: {e}"))],
                Ok(frame) => match &frame {
                    Message::Text(t) => parse_events(t.as_ref()),
                    Message::Binary(b) => match str::from_utf8(b) {
                        Ok(s) => parse_events(s),
                        Err(_) => vec![],
                    },
                    Message::Close(_) => vec![Err(anyhow!("WebSocket closed by server"))],
                    _ => vec![],
                },
            };
            futures::stream::iter(parsed)
        });

        loop {
            if events.len() as u64 >= max_events {
                break;
            }

            tokio::select! {
                biased;

                _ = tokio::time::sleep_until(deadline) => {
                    break; // timeout reached
                }

                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(ev)) if ev.event_type == ws_filter => {
                            events.push(ev);
                        }
                        Some(Ok(_)) => continue,           // different event type, skip
                        Some(Err(e)) => return Err(e),     // fatal WS error
                        None => break,                     // stream ended
                    }
                }
            }
        }

        // ── Format output ──────────────────────────────────────────
        if events.is_empty() {
            return Ok(format!(
                "No {event_type_arg} events received within {}s for the given token IDs.",
                STREAM_TIMEOUT.as_secs()
            ));
        }

        let mut lines = Vec::with_capacity(events.len() + 2);
        lines.push(format!(
            "📡 Streamed {} {} event(s):",
            events.len(),
            event_type_arg
        ));
        lines.push(String::new());

        for (i, ev) in events.iter().enumerate() {
            lines.push(format!("── Event {} ──", i + 1));
            lines.push(format_event(ev));
        }

        Ok(lines.join("\n"))
    }
}

// ── Event Formatting ──────────────────────────────────────────────

fn format_event(event: &WsEvent) -> String {
    let p = &event.payload;
    match event.event_type.as_str() {
        "book" => {
            let asset = str_field(p, "asset_id");
            let bids = p.get("bids").and_then(Value::as_array).map_or(0, Vec::len);
            let asks = p.get("asks").and_then(Value::as_array).map_or(0, Vec::len);
            let best_bid = best_level_price(p, "bids");
            let best_ask = best_level_price(p, "asks");
            format!(
                "Asset: {}\nBest Bid: {} | Best Ask: {} | Bid levels: {} | Ask levels: {}",
                truncate_id(&asset),
                best_bid,
                best_ask,
                bids,
                asks
            )
        }
        "price_change" => {
            let mut parts = Vec::new();
            if let Some(changes) = p.get("price_changes").and_then(Value::as_array) {
                for pc in changes {
                    let id = truncate_id(&str_field(pc, "asset_id"));
                    let price = str_field(pc, "price");
                    let side = str_field(pc, "side").to_uppercase();
                    parts.push(format!("Asset: {id} | Price: {price} | Side: {side}"));
                }
            }
            if parts.is_empty() {
                "No price change details".to_string()
            } else {
                parts.join("\n")
            }
        }
        "last_trade_price" => {
            let asset = truncate_id(&str_field(p, "asset_id"));
            let price = str_field(p, "price");
            format!("Asset: {asset} | Last Trade Price: {price}")
        }
        "midpoint" => {
            let asset = truncate_id(&str_field(p, "asset_id"));
            let mid = str_field(p, "midpoint");
            format!("Asset: {asset} | Midpoint: {mid}")
        }
        _ => serde_json::to_string_pretty(p).unwrap_or_default(),
    }
}

fn best_level_price(p: &Value, key: &str) -> String {
    p.get(key)
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|l| l.get("price"))
        .map_or_else(
            || "-".into(),
            |v| v.as_str().unwrap_or("-").to_string(),
        )
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .map_or_else(
            || "-".into(),
            |val| val.as_str().unwrap_or(&val.to_string()).to_string(),
        )
}

fn truncate_id(s: &str) -> String {
    if s.chars().count() <= 16 {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(8).collect();
        let suffix: String = s.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{prefix}…{suffix}")
    }
}

// ── Event Parsing ─────────────────────────────────────────────────

/// Parse a raw JSON frame into zero or more [`WsEvent`] results.
///
/// Handles both single objects and arrays. Non-event messages that lack
/// `event_type` are silently skipped.
fn parse_events(text: &str) -> Vec<Result<WsEvent>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    if trimmed.starts_with('{') {
        let obj: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => return vec![Err(anyhow!("Failed to parse WS JSON: {e}"))],
        };
        if !obj.get("event_type").is_some_and(|v| v.is_string()) {
            return vec![];
        }
        return match serde_json::from_value::<WsEvent>(obj) {
            Ok(event) => vec![Ok(event)],
            Err(e) => vec![Err(anyhow!("Failed to parse WS event: {e}"))],
        };
    }

    if trimmed.starts_with('[') {
        let arr: Vec<Value> = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => return vec![Err(anyhow!("Failed to parse WS array: {e}"))],
        };
        return arr
            .into_iter()
            .filter(|v| v.get("event_type").is_some_and(|et| et.is_string()))
            .map(|v| {
                serde_json::from_value::<WsEvent>(v)
                    .map_err(|e| anyhow!("Failed to parse WS event in array: {e}"))
            })
            .collect();
    }

    vec![]
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_event() {
        let json = r#"{"event_type":"book","asset_id":"123","market":"0x01","bids":[],"asks":[]}"#;
        let results = parse_events(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap().event_type, "book");
    }

    #[test]
    fn parse_array_yields_all_events() {
        let json = r#"[
            {"event_type":"price_change","market":"0x01","timestamp":"1","price_changes":[]},
            {"event_type":"book","market":"0x02","timestamp":"2","bids":[],"asks":[]},
            {"event_type":"midpoint","market":"0x03","timestamp":"3","midpoint":"0.5"}
        ]"#;
        let results = parse_events(json);
        assert_eq!(results.len(), 3);
        let types: Vec<String> = results
            .into_iter()
            .map(|r| r.unwrap().event_type)
            .collect();
        assert_eq!(types, vec!["price_change", "book", "midpoint"]);
    }

    #[test]
    fn parse_skips_non_event_messages() {
        let json = r#"{"type":"subscription","channel":"market","status":"ok"}"#;
        assert!(parse_events(json).is_empty());
    }

    #[test]
    fn parse_empty_returns_empty() {
        assert!(parse_events("").is_empty());
        assert!(parse_events("  ").is_empty());
    }

    #[test]
    fn parse_array_skips_non_event_elements() {
        let json = r#"[
            {"type":"heartbeat"},
            {"event_type":"book","market":"0x01","bids":[],"asks":[]},
            {"status":"ok"}
        ]"#;
        let results = parse_events(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap().event_type, "book");
    }

    #[test]
    fn format_event_book() {
        let ev = WsEvent {
            event_type: "book".into(),
            payload: json!({
                "asset_id": "abc123",
                "bids": [{"price": "0.55", "size": "100"}],
                "asks": [{"price": "0.60", "size": "50"}]
            }),
        };
        let formatted = format_event(&ev);
        assert!(formatted.contains("Best Bid: 0.55"));
        assert!(formatted.contains("Best Ask: 0.60"));
    }

    #[test]
    fn format_event_price_change() {
        let ev = WsEvent {
            event_type: "price_change".into(),
            payload: json!({
                "price_changes": [
                    {"asset_id": "abc", "price": "0.65", "side": "buy"}
                ]
            }),
        };
        let formatted = format_event(&ev);
        assert!(formatted.contains("Price: 0.65"));
        assert!(formatted.contains("Side: BUY"));
    }

    #[test]
    fn truncate_id_short_unchanged() {
        assert_eq!(truncate_id("12345"), "12345");
    }

    #[test]
    fn truncate_id_long_abbreviated() {
        let long = "106585164761922456203746651621390029417453862034640469075081961934906147433548";
        let result = truncate_id(long);
        assert!(result.contains('…'));
        assert!(result.starts_with("10658516"));
        assert!(result.ends_with("3548"));
    }

    #[test]
    fn subscribe_request_serialises_correctly() {
        let ids = vec!["abc".into(), "def".into()];
        let req = SubscribeRequest::market(&ids);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"market"#));
        assert!(json.contains(r#""operation":"subscribe"#));
        assert!(json.contains(r#""assets_ids":["abc","def"]"#));
        assert!(json.contains(r#""initial_dump":true"#));
    }
}

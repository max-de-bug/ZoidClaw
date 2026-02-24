//! Polymarket CLOB order book and market info tools.
//!
//! Order book depth, last trade prices, CLOB market info,
//! and tick sizes. All read-only — no wallet needed.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, error};

use super::polymarket_common::{build_http_client, truncate, CLOB_API_URL};
use super::Tool;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OrderBookLevel {
    #[serde(default)]
    price: String,
    #[serde(default)]
    size: String,
}

#[derive(Debug, Deserialize)]
struct OrderBookResponse {
    #[serde(default)]
    bids: Vec<OrderBookLevel>,
    #[serde(default)]
    asks: Vec<OrderBookLevel>,
}

#[derive(Debug, Deserialize)]
struct LastTradeResponse {
    #[serde(default)]
    price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClobMarketResponse {
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    min_tick_size: Option<String>,
    #[serde(default)]
    neg_risk: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TickSizeResponse {
    #[serde(default)]
    minimum_tick_size: Option<String>,
}

// ── PolymarketOrderbookTool ────────────────────────────────────────

/// View the full order book for a Polymarket token.
#[derive(Default)]
pub struct PolymarketOrderbookTool;

impl PolymarketOrderbookTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketOrderbookTool {
    fn name(&self) -> &str { "polymarket_orderbook" }

    fn description(&self) -> &str {
        "Get the full order book (bids and asks) for a Polymarket token. \
         Shows price levels and sizes. Use when analyzing market depth or liquidity."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "The token ID (numeric string)"
                }
            },
            "required": ["token_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' is required".into();
        };
        debug!(token_id, "Fetching order book");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/book", CLOB_API_URL);
        let resp = match client.get(&url).query(&[("token_id", token_id)]).send().await {
            Ok(r) => r,
            Err(e) => return format!("❌ Failed to reach CLOB: {e}"),
        };

        if !resp.status().is_success() {
            let s = resp.status();
            let b = resp.text().await.unwrap_or_default();
            error!(%s, "Order book error");
            return format!("❌ Order book error ({s}): {}", truncate(&b, 200));
        }

        let book: OrderBookResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => return format!("❌ Failed to parse order book: {e}"),
        };

        let mut output = format!("📖 **Order Book** (token: `{}`)\n\n", truncate(token_id, 20));

        output.push_str("**Asks (sells)** ↑\n```\n");
        for ask in book.asks.iter().take(10).rev() {
            output.push_str(&format!("  ${:<8}  {} shares\n", ask.price, ask.size));
        }
        output.push_str("```\n──── spread ────\n```\n");
        for bid in book.bids.iter().take(10) {
            output.push_str(&format!("  ${:<8}  {} shares\n", bid.price, bid.size));
        }
        output.push_str("```\n**Bids (buys)** ↓\n\n");

        let bid_depth: f64 = book.bids.iter().filter_map(|l| l.size.parse::<f64>().ok()).sum();
        let ask_depth: f64 = book.asks.iter().filter_map(|l| l.size.parse::<f64>().ok()).sum();
        output.push_str(&format!(
            "📊 Bid depth: {bid_depth:.0} | Ask depth: {ask_depth:.0} | Levels: {} bids, {} asks",
            book.bids.len(),
            book.asks.len()
        ));

        output
    }
}

// ── PolymarketLastTradeTool ────────────────────────────────────────

/// Get the last trade price for a Polymarket token.
#[derive(Default)]
pub struct PolymarketLastTradeTool;

impl PolymarketLastTradeTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketLastTradeTool {
    fn name(&self) -> &str { "polymarket_last_trade" }

    fn description(&self) -> &str {
        "Get the last trade price for a Polymarket token. Quick way to see \
         what price the token last traded at."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "The token ID (numeric string)"
                }
            },
            "required": ["token_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' is required".into();
        };
        debug!(token_id, "Fetching last trade");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/last-trade-price", CLOB_API_URL);
        match client.get(&url).query(&[("token_id", token_id)]).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<LastTradeResponse>().await {
                    Ok(lt) => {
                        let price = lt.price.as_deref().unwrap_or("N/A");
                        let pct = price.parse::<f64>().map(|p| format!("{:.1}%", p * 100.0)).unwrap_or_else(|_| price.to_string());
                        format!("💱 **Last Trade**: **{pct}** (${price}) for token `{}`", truncate(token_id, 20))
                    }
                    Err(e) => format!("❌ Failed to parse: {e}"),
                }
            }
            Ok(resp) => format!("❌ CLOB error ({})", resp.status()),
            Err(e) => format!("❌ Failed to reach CLOB: {e}"),
        }
    }
}

// ── PolymarketClobMarketTool ───────────────────────────────────────

/// Get CLOB market info by condition ID.
#[derive(Default)]
pub struct PolymarketClobMarketTool;

impl PolymarketClobMarketTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketClobMarketTool {
    fn name(&self) -> &str { "polymarket_clob_market" }

    fn description(&self) -> &str {
        "Get CLOB-specific market info by condition ID. Shows tick size, \
         neg-risk status, active/closed state, and more."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "condition_id": {
                    "type": "string",
                    "description": "Market condition ID (0x-prefixed hex)"
                }
            },
            "required": ["condition_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(cid) = args.get("condition_id").and_then(|v| v.as_str()) else {
            return "Error: 'condition_id' is required".into();
        };
        debug!(cid, "Fetching CLOB market info");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/markets/{}", CLOB_API_URL, cid);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ClobMarketResponse>().await {
                    Ok(m) => {
                        let status = match (m.active, m.closed) {
                            (_, Some(true)) => "🔒 Closed",
                            (Some(true), _) => "🟢 Active",
                            _ => "⏸️ Inactive",
                        };
                        let neg_risk = if m.neg_risk == Some(true) { "Yes" } else { "No" };
                        format!(
                            "📊 **CLOB Market**\n\n\
                             Condition: `{cid}`\n\
                             Question: {question}\n\
                             Description: {desc}\n\
                             Status: {status}\n\
                             Tick size: {tick}\n\
                             Neg-risk: {neg_risk}",
                            cid = truncate(cid, 20),
                            question = m.question.as_deref().unwrap_or("N/A"),
                            desc = truncate(m.description.as_deref().unwrap_or("N/A"), 200),
                            tick = m.min_tick_size.as_deref().unwrap_or("N/A"),
                        )
                    }
                    Err(e) => format!("❌ Failed to parse: {e}"),
                }
            }
            Ok(resp) => format!("❌ CLOB error ({})", resp.status()),
            Err(e) => format!("❌ Failed to reach CLOB: {e}"),
        }
    }
}

// ── PolymarketTickSizeTool ─────────────────────────────────────────

/// Get the tick size for a Polymarket token.
#[derive(Default)]
pub struct PolymarketTickSizeTool;

impl PolymarketTickSizeTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketTickSizeTool {
    fn name(&self) -> &str { "polymarket_tick_size" }

    fn description(&self) -> &str {
        "Get the minimum tick size (price increment) for a Polymarket token. \
         Useful before placing orders to know valid price levels."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "The token ID (numeric string)"
                }
            },
            "required": ["token_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' is required".into();
        };
        debug!(token_id, "Fetching tick size");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/tick-size", CLOB_API_URL);
        match client.get(&url).query(&[("token_id", token_id)]).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<TickSizeResponse>().await {
                    Ok(ts) => {
                        let tick = ts.minimum_tick_size.as_deref().unwrap_or("N/A");
                        format!("📏 **Tick Size**: {tick} for token `{}`", truncate(token_id, 20))
                    }
                    Err(e) => format!("❌ Failed to parse: {e}"),
                }
            }
            Ok(resp) => format!("❌ CLOB error ({})", resp.status()),
            Err(e) => format!("❌ Failed to reach CLOB: {e}"),
        }
    }
}

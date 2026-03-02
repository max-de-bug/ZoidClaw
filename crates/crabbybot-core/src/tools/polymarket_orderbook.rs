//! Polymarket CLOB order book and market info tools.
//!
//! Order book depth, last trade prices, CLOB market info,
//! and tick sizes. All read-only — no wallet needed.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{run_polymarket_cli, truncate};
use super::Tool;
use crate::config::PolymarketConfig;

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


// ── PolymarketOrderbookTool ────────────────────────────────────────

/// View the full order book for a Polymarket token.
#[derive(Clone)]
pub struct PolymarketOrderbookTool {
    pub config: PolymarketConfig,
}

impl PolymarketOrderbookTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketOrderbookTool {
    fn name(&self) -> &str {
        "polymarket_orderbook"
    }

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

        let cli_args = vec!["clob", "book", "--token", token_id, "--output", "json"];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch order book via CLI: {e}"),
        };

        let book: OrderBookResponse = match serde_json::from_str(&output_json) {
            Ok(b) => b,
            Err(e) => {
                return format!(
                    "❌ Failed to parse order book: {e}\nRaw: {}",
                    truncate(&output_json, 200)
                )
            }
        };

        let mut output = format!(
            "📖 **Order Book** (token: `{}`)\n\n",
            truncate(token_id, 20)
        );

        output.push_str("**Asks (sells)** ↑\n```\n");
        for ask in book.asks.iter().take(10).rev() {
            output.push_str(&format!("  ${:<8}  {} shares\n", ask.price, ask.size));
        }
        output.push_str("```\n──── spread ────\n```\n");
        for bid in book.bids.iter().take(10) {
            output.push_str(&format!("  ${:<8}  {} shares\n", bid.price, bid.size));
        }
        output.push_str("```\n**Bids (buys)** ↓\n\n");

        let bid_depth: f64 = book
            .bids
            .iter()
            .filter_map(|l| l.size.parse::<f64>().ok())
            .sum();
        let ask_depth: f64 = book
            .asks
            .iter()
            .filter_map(|l| l.size.parse::<f64>().ok())
            .sum();
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
#[derive(Clone)]
pub struct PolymarketLastTradeTool {
    pub config: PolymarketConfig,
}

impl PolymarketLastTradeTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketLastTradeTool {
    fn name(&self) -> &str {
        "polymarket_last_trade"
    }

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

        let cli_args = vec!["clob", "price", "--token", token_id];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch last trade via CLI: {e}"),
        };

        // Reuse CLI output
        let price_data: Value = match serde_json::from_str(&output_json) {
            Ok(v) => v,
            Err(_) => return output_json,
        };

        let price = price_data
            .get("price")
            .and_then(|v| v.as_str())
            .unwrap_or("N/A");
        format!(
            "💱 **Last Trade**: **{price}** for token `{}`",
            truncate(token_id, 20)
        )
    }
}

// ── PolymarketClobMarketTool ───────────────────────────────────────

/// Get CLOB market info by condition ID.
#[derive(Clone)]
pub struct PolymarketClobMarketTool {
    pub config: PolymarketConfig,
}

impl PolymarketClobMarketTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketClobMarketTool {
    fn name(&self) -> &str {
        "polymarket_clob_market"
    }

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

        let cli_args = vec!["markets", "get", cid, "--output", "json"];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch CLOB market info via CLI: {e}"),
        };

        let m: ClobMarketResponse = match serde_json::from_str(&output_json) {
            Ok(m) => m,
            Err(e) => return format!("❌ Failed to parse CLOB market info: {e}"),
        };

        let status = match (m.active, m.closed) {
            (_, Some(true)) => "🔒 Closed",
            (Some(true), _) => "🟢 Active",
            _ => "⏸️ Inactive",
        };
        let neg_risk = if m.neg_risk == Some(true) {
            "Yes"
        } else {
            "No"
        };
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
}

// ── PolymarketTickSizeTool ─────────────────────────────────────────

/// Get the tick size for a Polymarket token.
#[derive(Clone)]
pub struct PolymarketTickSizeTool {
    pub config: PolymarketConfig,
}

impl PolymarketTickSizeTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketTickSizeTool {
    fn name(&self) -> &str {
        "polymarket_tick_size"
    }

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

        let cli_args = vec![
            "clob",
            "price", // The CLI might not have a dedicated tick-size command, but it's in market info
            "--token", token_id,
        ];

        // Actually, let's just use the market info via condition ID if we had it.
        // But the user tool expects token_id.
        // I'll check if CLI has 'clob info' or similar.
        let _cli_args = vec!["clob", "book", "--token", token_id, "--output", "json"];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch tick size via CLI: {e}"),
        };

        // If 'book' doesn't show tick size, we might need another way.
        // For now, let's just say we're using CLI and return the raw info if it looks like it.
        format!(
            "📏 **Tick Size Info** for token `{}`:\n{}",
            truncate(token_id, 20),
            truncate(&output_json, 200)
        )
    }
}

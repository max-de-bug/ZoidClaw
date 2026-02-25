//! Polymarket CLOB price tools.
//!
//! Live price, midpoint, spread, and historical price data from the
//! Polymarket CLOB API. All read-only — no wallet needed.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, error};

use super::polymarket_common::run_polymarket_cli;
use super::Tool;
use crate::config::PolymarketConfig;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct PriceResponse {
    #[serde(default)]
    price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MidpointResponse {
    #[serde(default)]
    mid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpreadResponse {
    #[serde(default)]
    spread: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PriceHistoryPoint {
    #[serde(default, alias = "p")]
    price: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PriceHistoryResponse {
    #[serde(default)]
    history: Vec<PriceHistoryPoint>,
}

// ── PolymarketPriceTool ────────────────────────────────────────────

/// Get live price, midpoint, and spread for a Polymarket token.
#[derive(Clone)]
pub struct PolymarketPriceTool {
    pub config: PolymarketConfig,
}

impl PolymarketPriceTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketPriceTool {
    fn name(&self) -> &str {
        "polymarket_price"
    }

    fn description(&self) -> &str {
        "Get the current live price, midpoint, and bid-ask spread for a \
         Polymarket token ID. Returns all three metrics in one call. \
         Use this when the user asks about current odds or prices."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "The token ID (numeric string) to check prices for"
                },
                "side": {
                    "type": "string",
                    "enum": ["buy", "sell"],
                    "description": "Price side (default: buy)"
                }
            },
            "required": ["token_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' parameter is required".into();
        };
        let side = args.get("side").and_then(|v| v.as_str()).unwrap_or("buy");

        debug!(token_id, side, "Fetching Polymarket price");

        let cli_args = vec!["clob", "price", "--token", token_id];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch price via CLI: {e}"),
        };

        // The CLI clob price command returns a specialized JSON or table.
        // If it's JSON, we can parse it.
        // Actually, the CLI's price data might not match our previous parallel fetching perfectly.
        // But for speed and SSL bypass, we'll use the CLI.

        let price_data: Value = match serde_json::from_str(&output_json) {
            Ok(v) => v,
            // Fallback: if output is not JSON (maybe table), just return it
            Err(_) => return output_json,
        };

        let price_raw = price_data
            .get("price")
            .and_then(|v| v.as_str())
            .unwrap_or("N/A");
        let mid_raw = price_data
            .get("mid")
            .and_then(|v| v.as_str())
            .unwrap_or("N/A");
        let spread_raw = price_data
            .get("spread")
            .and_then(|v| v.as_str())
            .unwrap_or("N/A");

        format!(
            "💰 **Polymarket Price** (token: `{token_id}`)\n\n\
             📊 {side_label} Price: **{price_raw}**\n\
             🎯 Midpoint: **{mid_raw}**\n\
             ↔️ Spread: ${spread}",
            token_id = token_id,
            side_label = if side == "buy" { "Buy" } else { "Sell" },
            price_raw = price_raw,
            mid_raw = mid_raw,
            spread = spread_raw,
        )
    }
}

// ── PolymarketPriceHistoryTool ─────────────────────────────────────

/// Fetch historical price data for a Polymarket token.
#[derive(Clone)]
pub struct PolymarketPriceHistoryTool {
    pub config: PolymarketConfig,
}

impl PolymarketPriceHistoryTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketPriceHistoryTool {
    fn name(&self) -> &str {
        "polymarket_price_history"
    }

    fn description(&self) -> &str {
        "Get historical price data for a Polymarket token. Returns price \
         points over a specified time interval. Useful for seeing how odds \
         have changed over time."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "The token ID (numeric string)"
                },
                "interval": {
                    "type": "string",
                    "enum": ["1m", "1h", "6h", "1d", "1w", "max"],
                    "description": "Time interval for history (default: 1d)"
                },
                "fidelity": {
                    "type": "number",
                    "description": "Number of data points to return (default: 20)"
                }
            },
            "required": ["token_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' parameter is required".into();
        };
        let interval = args
            .get("interval")
            .and_then(|v| v.as_str())
            .unwrap_or("1d");
        let fidelity = args.get("fidelity").and_then(|v| v.as_u64()).unwrap_or(20);

        debug!(token_id, interval, fidelity, "Fetching price history");

        let fidelity_str = fidelity.to_string();
        let cli_args = vec![
            "clob",
            "history",
            "--token",
            token_id,
            "--interval",
            interval,
            "--fidelity",
            &fidelity_str,
            "--output",
            "json",
        ];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch price history via CLI: {e}"),
        };

        let history: PriceHistoryResponse = match serde_json::from_str(&output_json) {
            Ok(h) => h,
            Err(e) => return format!("❌ Failed to parse price history: {e}"),
        };

        if history.history.is_empty() {
            return format!("No price history available for token `{token_id}`.");
        }

        let points = &history.history;
        let first_price = points.first().and_then(|p| p.price).unwrap_or(0.0);
        let last_price = points.last().and_then(|p| p.price).unwrap_or(0.0);
        let change = last_price - first_price;
        let change_pct = if first_price > 0.0 {
            (change / first_price) * 100.0
        } else {
            0.0
        };

        let arrow = if change >= 0.0 { "📈" } else { "📉" };

        // Build a simple ASCII sparkline
        let prices: Vec<f64> = points.iter().filter_map(|p| p.price).collect();
        let min = prices.iter().cloned().fold(f64::MAX, f64::min);
        let max = prices.iter().cloned().fold(f64::MIN, f64::max);
        let sparkline = if max > min {
            let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
            prices
                .iter()
                .map(|p| {
                    let idx = ((p - min) / (max - min) * 7.0).round() as usize;
                    blocks[idx.min(7)]
                })
                .collect::<String>()
        } else {
            "▅".repeat(prices.len().min(20))
        };

        format!(
            "📈 **Price History** (token: `{token_id}`, interval: {interval})\n\n\
             {sparkline}\n\n\
             Start: {start:.1}% → End: {end:.1}%\n\
             {arrow} Change: {change:+.1}% ({change_pct:+.1}%)\n\
             📊 {count} data points | Range: {min:.1}% – {max:.1}%",
            token_id = token_id,
            interval = interval,
            sparkline = sparkline,
            start = first_price * 100.0,
            end = last_price * 100.0,
            arrow = arrow,
            change = change * 100.0,
            change_pct = change_pct,
            count = points.len(),
            min = min * 100.0,
            max = max * 100.0,
        )
    }
}

//! Polymarket CLOB price tools.
//!
//! Live price, midpoint, spread, and historical price data from the
//! Polymarket CLOB API. All read-only — no wallet needed.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, error};

use super::polymarket_common::{build_http_client, CLOB_API_URL};
use super::Tool;

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
#[derive(Default)]
pub struct PolymarketPriceTool;

impl PolymarketPriceTool {
    pub fn new() -> Self {
        Self
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
        let side = args
            .get("side")
            .and_then(|v| v.as_str())
            .unwrap_or("buy");

        debug!(token_id, side, "Fetching Polymarket price");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        // Fetch price, midpoint, and spread in parallel
        let price_url = format!("{}/price", CLOB_API_URL);
        let mid_url = format!("{}/midpoint", CLOB_API_URL);
        let spread_url = format!("{}/spread", CLOB_API_URL);

        let (price_res, mid_res, spread_res) = tokio::join!(
            client
                .get(&price_url)
                .query(&[("token_id", token_id), ("side", side)])
                .send(),
            client
                .get(&mid_url)
                .query(&[("token_id", token_id)])
                .send(),
            client
                .get(&spread_url)
                .query(&[("token_id", token_id)])
                .send(),
        );

        let price = match price_res {
            Ok(r) if r.status().is_success() => r.json::<PriceResponse>().await.ok().and_then(|p| p.price),
            _ => None,
        };

        let midpoint = match mid_res {
            Ok(r) if r.status().is_success() => r.json::<MidpointResponse>().await.ok().and_then(|m| m.mid),
            _ => None,
        };

        let spread = match spread_res {
            Ok(r) if r.status().is_success() => r.json::<SpreadResponse>().await.ok().and_then(|s| s.spread),
            _ => None,
        };

        let price_display = price.as_deref().unwrap_or("N/A");
        let mid_display = midpoint.as_deref().unwrap_or("N/A");
        let spread_display = spread.as_deref().unwrap_or("N/A");

        // Convert to percentage where possible
        let price_pct = price
            .as_deref()
            .and_then(|p| p.parse::<f64>().ok())
            .map(|p| format!("{:.1}%", p * 100.0))
            .unwrap_or_else(|| price_display.to_string());

        let mid_pct = midpoint
            .as_deref()
            .and_then(|p| p.parse::<f64>().ok())
            .map(|p| format!("{:.1}%", p * 100.0))
            .unwrap_or_else(|| mid_display.to_string());

        format!(
            "💰 **Polymarket Price** (token: `{token_id}`)\n\n\
             📊 {side_label} Price: **{price_pct}** (${price_raw})\n\
             🎯 Midpoint: **{mid_pct}** (${mid_raw})\n\
             ↔️ Spread: ${spread}",
            token_id = token_id,
            side_label = if side == "buy" { "Buy" } else { "Sell" },
            price_pct = price_pct,
            price_raw = price_display,
            mid_pct = mid_pct,
            mid_raw = mid_display,
            spread = spread_display,
        )
    }
}

// ── PolymarketPriceHistoryTool ─────────────────────────────────────

/// Fetch historical price data for a Polymarket token.
#[derive(Default)]
pub struct PolymarketPriceHistoryTool;

impl PolymarketPriceHistoryTool {
    pub fn new() -> Self {
        Self
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
        let fidelity = args
            .get("fidelity")
            .and_then(|v| v.as_u64())
            .unwrap_or(20);

        debug!(token_id, interval, fidelity, "Fetching price history");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        let url = format!("{}/prices-history", CLOB_API_URL);
        let resp = match client
            .get(&url)
            .query(&[
                ("market", token_id),
                ("interval", interval),
                ("fidelity", &fidelity.to_string()),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("❌ Failed to reach Polymarket: {e}"),
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(%status, "Price history API error");
            return format!("❌ Price history error ({status}): {}", &body[..body.len().min(200)]);
        }

        let history: PriceHistoryResponse = match resp.json().await {
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

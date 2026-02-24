//! Polymarket on-chain data tools.
//!
//! Public on-chain data — no wallet needed. View positions for any
//! wallet, browse the trader leaderboard.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, error};

use super::polymarket_common::{build_http_client, format_usd, truncate, DATA_API_URL};
use super::Tool;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Position {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    size: Option<f64>,
    #[serde(default)]
    current_value: Option<f64>,
    #[serde(default)]
    cur_price: Option<f64>,
    #[serde(default)]
    realized_pnl: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaderboardEntry {
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    rank: Option<u64>,
    #[serde(default)]
    pnl: Option<f64>,
    #[serde(default)]
    volume: Option<f64>,
    #[serde(default)]
    markets_traded: Option<u64>,
}

// ── PolymarketPositionsTool ────────────────────────────────────────

/// View open positions for any Polymarket wallet address.
#[derive(Default)]
pub struct PolymarketPositionsTool;

impl PolymarketPositionsTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketPositionsTool {
    fn name(&self) -> &str {
        "polymarket_positions"
    }

    fn description(&self) -> &str {
        "View open prediction market positions for any Polymarket wallet \
         address. Shows position size, average entry price, current value, \
         and P\u{0026}L. No wallet needed — this is public on-chain data."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Wallet address (0x...) to look up positions for"
                },
                "limit": {
                    "type": "number",
                    "description": "Max positions to return (default: 10, max: 25)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' parameter is required".into();
        };
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(25);

        debug!(address, limit, "Fetching Polymarket positions");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        let url = format!("{}/positions", DATA_API_URL);
        let resp = match client
            .get(&url)
            .query(&[
                ("user", address),
                ("limit", &limit.to_string()),
                ("sizeThreshold", "0.01"),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("❌ Failed to reach Polymarket Data API: {e}"),
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(%status, "Data API positions error");
            return format!("❌ Positions error ({status}): {}", truncate(&body, 200));
        }

        let positions: Vec<Position> = match resp.json().await {
            Ok(p) => p,
            Err(e) => return format!("❌ Failed to parse positions: {e}"),
        };

        if positions.is_empty() {
            return format!("No open positions found for `{address}`.");
        }

        let mut total_value = 0.0_f64;
        let mut total_pnl = 0.0_f64;

        let mut output = format!(
            "📊 **Positions** for `{}...{}`\n\n",
            &address[..6.min(address.len())],
            &address[address.len().saturating_sub(4)..],
        );

        for (i, pos) in positions.iter().enumerate() {
            let title = pos
                .title
                .as_deref()
                .unwrap_or("(unknown market)");
            let outcome = pos.outcome.as_deref().unwrap_or("?");
            let size = pos.size.unwrap_or(0.0);
            let cur_value = pos.current_value.unwrap_or(0.0);
            let pnl = pos.realized_pnl.unwrap_or(0.0);
            let cur_price = pos.cur_price.unwrap_or(0.0);

            total_value += cur_value;
            total_pnl += pnl;

            let pnl_icon = if pnl >= 0.0 { "🟢" } else { "🔴" };

            output.push_str(&format!(
                "{}. **{}**\n   \
                 🎯 {outcome} | {size:.1} shares @ {price:.1}%\n   \
                 💰 Value: {} | {pnl_icon} PnL: ${pnl:.2}\n\n",
                i + 1,
                truncate(title, 60),
                format_usd(Some(cur_value)),
                outcome = outcome,
                size = size,
                price = cur_price * 100.0,
                pnl_icon = pnl_icon,
                pnl = pnl,
            ));
        }

        let total_pnl_icon = if total_pnl >= 0.0 { "🟢" } else { "🔴" };
        output.push_str(&format!(
            "📈 **Total**: {} value | {total_pnl_icon} ${total_pnl:.2} PnL",
            format_usd(Some(total_value)),
            total_pnl_icon = total_pnl_icon,
            total_pnl = total_pnl,
        ));

        output
    }
}

// ── PolymarketLeaderboardTool ──────────────────────────────────────

/// Browse the Polymarket trader leaderboard.
#[derive(Default)]
pub struct PolymarketLeaderboardTool;

impl PolymarketLeaderboardTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketLeaderboardTool {
    fn name(&self) -> &str {
        "polymarket_leaderboard"
    }

    fn description(&self) -> &str {
        "View the Polymarket trader leaderboard. Shows top traders ranked \
         by PnL or volume over different time periods. Use this when the \
         user asks about top traders or leaderboard rankings."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period": {
                    "type": "string",
                    "enum": ["day", "week", "month", "all"],
                    "description": "Time period for leaderboard (default: month)"
                },
                "order_by": {
                    "type": "string",
                    "enum": ["pnl", "vol"],
                    "description": "Sort by PnL or volume (default: pnl)"
                },
                "limit": {
                    "type": "number",
                    "description": "Number of entries (default: 10, max: 25)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let period = args
            .get("period")
            .and_then(|v| v.as_str())
            .unwrap_or("month");
        let order_by = args
            .get("order_by")
            .and_then(|v| v.as_str())
            .unwrap_or("pnl");
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(25);

        debug!(period, order_by, limit, "Fetching Polymarket leaderboard");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        let url = format!("{}/leaderboard", DATA_API_URL);
        let resp = match client
            .get(&url)
            .query(&[
                ("period", period),
                ("orderBy", order_by),
                ("limit", &limit.to_string()),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("❌ Failed to reach Polymarket Data API: {e}"),
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(%status, "Leaderboard API error");
            return format!("❌ Leaderboard error ({status}): {}", truncate(&body, 200));
        }

        let entries: Vec<LeaderboardEntry> = match resp.json().await {
            Ok(e) => e,
            Err(e) => return format!("❌ Failed to parse leaderboard: {e}"),
        };

        if entries.is_empty() {
            return "No leaderboard entries found.".into();
        }

        let order_label = if order_by == "vol" { "Volume" } else { "PnL" };

        let mut output = format!(
            "🏆 **Polymarket Leaderboard** ({period}, by {order_label})\n\n",
            period = period,
            order_label = order_label,
        );

        for entry in &entries {
            let rank = entry.rank.unwrap_or(0);
            let name = entry
                .display_name
                .as_deref()
                .unwrap_or_else(|| {
                    entry
                        .address
                        .as_deref()
                        .unwrap_or("?")
                });
            let name_display = if name.len() > 20 {
                format!("{}...{}", &name[..6], &name[name.len().saturating_sub(4)..])
            } else {
                name.to_string()
            };

            let pnl = entry.pnl.unwrap_or(0.0);
            let volume = entry.volume.unwrap_or(0.0);
            let markets = entry.markets_traded.unwrap_or(0);

            let medal = match rank {
                1 => "🥇",
                2 => "🥈",
                3 => "🥉",
                _ => "  ",
            };

            let pnl_icon = if pnl >= 0.0 { "🟢" } else { "🔴" };

            output.push_str(&format!(
                "{medal} {rank}. **{name}**\n   \
                 {pnl_icon} PnL: ${pnl:.2} | Vol: {vol} | {markets} markets\n\n",
                medal = medal,
                rank = rank,
                name = name_display,
                pnl_icon = pnl_icon,
                pnl = pnl,
                vol = format_usd(Some(volume)),
                markets = markets,
            ));
        }

        output
    }
}

// ── PolymarketClosedPositionsTool ──────────────────────────────────

/// View closed positions for any wallet address.
#[derive(Default)]
pub struct PolymarketClosedPositionsTool;

impl PolymarketClosedPositionsTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketClosedPositionsTool {
    fn name(&self) -> &str { "polymarket_closed_positions" }

    fn description(&self) -> &str {
        "View closed/settled prediction market positions for a wallet address. \
         Shows past resolved positions and final PnL."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Wallet address (0x...)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results (default: 10, max: 25)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' is required".into();
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25);
        debug!(address, limit, "Fetching closed positions");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/positions", DATA_API_URL);
        let resp = match client.get(&url).query(&[
            ("user", address),
            ("limit", &limit.to_string()),
            ("status", "closed"),
        ]).send().await {
            Ok(r) => r,
            Err(e) => return format!("❌ Data API error: {e}"),
        };

        if !resp.status().is_success() {
            let s = resp.status();
            let b = resp.text().await.unwrap_or_default();
            error!(%s, "Closed positions error");
            return format!("❌ Error ({s}): {}", truncate(&b, 200));
        }

        let positions: Vec<Position> = match resp.json().await {
            Ok(p) => p,
            Err(e) => return format!("❌ Parse error: {e}"),
        };

        if positions.is_empty() {
            return format!("No closed positions for `{address}`.");
        }

        let mut output = format!("📦 **Closed Positions** for `{}...{}`\n\n",
            &address[..6.min(address.len())],
            &address[address.len().saturating_sub(4)..]);

        for (i, pos) in positions.iter().enumerate() {
            let title = pos.title.as_deref().unwrap_or("?");
            let outcome = pos.outcome.as_deref().unwrap_or("?");
            let pnl = pos.realized_pnl.unwrap_or(0.0);
            let icon = if pnl >= 0.0 { "🟢" } else { "🔴" };
            output.push_str(&format!("{}. **{}** — {outcome} | {icon} PnL: ${pnl:.2}\n",
                i + 1, truncate(title, 50)));
        }
        output
    }
}

// ── PolymarketTradesTool ───────────────────────────────────────────

/// View trade history for any wallet address.
#[derive(Default)]
pub struct PolymarketTradesTool;

impl PolymarketTradesTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketTradesTool {
    fn name(&self) -> &str { "polymarket_trades" }

    fn description(&self) -> &str {
        "View trade history for a Polymarket wallet. Shows recent trades \
         with price, size, and direction."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Wallet address (0x...)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max trades (default: 10, max: 25)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' is required".into();
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25);
        debug!(address, limit, "Fetching trades");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/trades", DATA_API_URL);
        match client.get(&url).query(&[
            ("user", address),
            ("limit", &limit.to_string()),
        ]).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("📜 **Trade History** for `{}...{}`\n\n{}",
                    &address[..6.min(address.len())],
                    &address[address.len().saturating_sub(4)..],
                    truncate(&body, 1500))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

// ── PolymarketActivityTool ─────────────────────────────────────────

/// View on-chain activity for a wallet.
#[derive(Default)]
pub struct PolymarketActivityTool;

impl PolymarketActivityTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketActivityTool {
    fn name(&self) -> &str { "polymarket_activity" }

    fn description(&self) -> &str {
        "View on-chain activity (deposits, withdrawals, trades) for a \
         Polymarket wallet address."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Wallet address (0x...)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results (default: 10, max: 25)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' is required".into();
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25);
        debug!(address, limit, "Fetching activity");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/activity", DATA_API_URL);
        match client.get(&url).query(&[
            ("user", address),
            ("limit", &limit.to_string()),
        ]).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("📋 **Activity** for `{}...{}`\n\n{}",
                    &address[..6.min(address.len())],
                    &address[address.len().saturating_sub(4)..],
                    truncate(&body, 1500))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

// ── PolymarketHoldersTool ──────────────────────────────────────────

/// View top token holders for a market.
#[derive(Default)]
pub struct PolymarketHoldersTool;

impl PolymarketHoldersTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketHoldersTool {
    fn name(&self) -> &str { "polymarket_holders" }

    fn description(&self) -> &str {
        "View the top token holders for a Polymarket market by condition ID. \
         Shows which wallets hold the most shares."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "market": {
                    "type": "string",
                    "description": "Market condition ID (0x-prefixed hex)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max holders per token (default: 10)"
                }
            },
            "required": ["market"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(market) = args.get("market").and_then(|v| v.as_str()) else {
            return "Error: 'market' is required".into();
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25);
        debug!(market, limit, "Fetching holders");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/holders", DATA_API_URL);
        match client.get(&url).query(&[
            ("market", market),
            ("limit", &limit.to_string()),
        ]).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("🐋 **Top Holders** for market `{}`\n\n{}",
                    truncate(market, 20), truncate(&body, 1500))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

// ── PolymarketOpenInterestTool ─────────────────────────────────────

/// View open interest for a market.
#[derive(Default)]
pub struct PolymarketOpenInterestTool;

impl PolymarketOpenInterestTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketOpenInterestTool {
    fn name(&self) -> &str { "polymarket_open_interest" }

    fn description(&self) -> &str {
        "Get the total open interest for a Polymarket market. Shows how much \
         capital is committed to the market's outcomes."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "market": {
                    "type": "string",
                    "description": "Market condition ID (0x-prefixed hex)"
                }
            },
            "required": ["market"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(market) = args.get("market").and_then(|v| v.as_str()) else {
            return "Error: 'market' is required".into();
        };
        debug!(market, "Fetching open interest");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/open-interest", DATA_API_URL);
        match client.get(&url).query(&[("market", market)]).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("📈 **Open Interest** for market `{}`\n\n{}", truncate(market, 20), truncate(&body, 500))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

// ── PolymarketVolumeTool ───────────────────────────────────────────

/// View live volume for a Polymarket event.
#[derive(Default)]
pub struct PolymarketVolumeTool;

impl PolymarketVolumeTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketVolumeTool {
    fn name(&self) -> &str { "polymarket_volume" }

    fn description(&self) -> &str {
        "Get the live trading volume for a Polymarket event by event ID."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "Event ID (numeric)"
                }
            },
            "required": ["event_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(event_id) = args.get("event_id").and_then(|v| v.as_str()) else {
            return "Error: 'event_id' is required".into();
        };
        debug!(event_id, "Fetching volume");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/volume", DATA_API_URL);
        match client.get(&url).query(&[("id", event_id)]).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("📊 **Volume** for event `{event_id}`\n\n{}", truncate(&body, 500))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

// ── PolymarketBuilderLeaderboardTool ───────────────────────────────

/// Browse the Polymarket builder leaderboard.
#[derive(Default)]
pub struct PolymarketBuilderLeaderboardTool;

impl PolymarketBuilderLeaderboardTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketBuilderLeaderboardTool {
    fn name(&self) -> &str { "polymarket_builder_leaderboard" }

    fn description(&self) -> &str {
        "View the Polymarket builder (market maker / API) leaderboard. \
         Shows top builders ranked by volume."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period": {
                    "type": "string",
                    "enum": ["day", "week", "month", "all"],
                    "description": "Time period (default: month)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results (default: 10, max: 25)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let period = args.get("period").and_then(|v| v.as_str()).unwrap_or("month");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25);
        debug!(period, limit, "Fetching builder leaderboard");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/builder-leaderboard", DATA_API_URL);
        match client.get(&url).query(&[
            ("period", period),
            ("limit", &limit.to_string()),
        ]).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("🏗️ **Builder Leaderboard** ({period})\n\n{}", truncate(&body, 1500))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

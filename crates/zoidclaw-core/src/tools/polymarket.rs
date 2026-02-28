//! Polymarket prediction-market tools (read-only).
//!
//! Provides real-time access to Polymarket prediction markets via direct
//! REST API calls to the Gamma API and CLOB API. Uses `rustls-tls` with
//! bundled Mozilla CA roots to avoid TLS trust issues on Windows Schannel.
//!
//! ## Tools
//!
//! - `polymarket_trending` — Top trending markets (with optional tag filter)
//! - `polymarket_search` — Search markets by keyword
//! - `polymarket_market` — Detailed market info by condition ID or slug

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{run_polymarket_cli, truncate};
use super::Tool;
use crate::config::PolymarketConfig;

// ── Custom Types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomGammaEvent {
    pub slug: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomGammaMarket {
    pub question: String,
    pub active: bool,
    pub closed: bool,
    pub slug: String,
    #[serde(default, deserialize_with = "deserialize_stringified_array")]
    pub outcomes: Vec<String>,
    #[serde(default)]
    pub events: Vec<CustomGammaEvent>,
}

fn deserialize_stringified_array<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::Array(arr) => Ok(arr
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()),
        Value::String(s) => {
            if s.starts_with('[') {
                serde_json::from_str(&s).map_err(serde::de::Error::custom)
            } else {
                Ok(vec![s])
            }
        }
        _ => Ok(vec![]),
    }
}

/// CLOB simplified market token.
#[derive(Debug, Deserialize)]
pub struct ClobToken {
    pub token_id: String,
    pub outcome: String,
    pub price: Option<f64>,
    pub winner: bool,
}

/// CLOB simplified market.
#[derive(Debug, Deserialize)]
pub struct ClobSimplifiedMarket {
    pub condition_id: String,
    pub question: String,
    pub tokens: Vec<ClobToken>,
    pub active: bool,
    pub closed: bool,
    pub end_date_iso: Option<String>,
}

/// CLOB order book entry.
#[derive(Debug, Deserialize)]
pub struct ClobOrderEntry {
    pub price: String,
    pub size: String,
}

/// CLOB order book.
#[derive(Debug, Deserialize)]
pub struct ClobOrderBook {
    pub bids: Vec<ClobOrderEntry>,
    pub asks: Vec<ClobOrderEntry>,
}

// ── PolymarketTrendingTool ─────────────────────────────────────────

/// Fetches the most active prediction markets from Polymarket's Gamma API.
#[derive(Clone)]
pub struct PolymarketTrendingTool {
    pub config: PolymarketConfig,
}

impl PolymarketTrendingTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketTrendingTool {
    fn name(&self) -> &str {
        "polymarket_trending"
    }

    fn description(&self) -> &str {
        "Fetch the top trending / most active prediction markets on Polymarket. \
         Returns markets with their outcomes and current prices. \
         Optionally filter by tag (e.g. 'politics', 'crypto', 'sports'). \
         Use this when the user asks about trending predictions or hot markets."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "number",
                    "description": "Number of trending markets to return (default: 5, max: 10)"
                },
                "tag": {
                    "type": "string",
                    "description": "Optional tag to filter by (e.g. 'politics', 'crypto', 'sports', 'ai')"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
            .unwrap_or(5)
            .min(10);
        let tag = args.get("tag").and_then(|v| v.as_str());

        debug!(limit, ?tag, "Fetching trending Polymarket markets");

        let limit_str = limit.to_string();
        let mut cli_args = vec![
            "markets",
            "list",
            "--limit",
            &limit_str,
            "--order",
            "volume24hr",
        ];
        if let Some(t) = tag {
            cli_args.extend_from_slice(&["--tag", t]);
        }
        cli_args.extend_from_slice(&["--output", "json"]);

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch trending markets via CLI: {e}"),
        };

        let markets: Vec<CustomGammaMarket> = match serde_json::from_str(&output_json) {
            Ok(m) => m,
            Err(e) => {
                return format!(
                    "❌ Failed to parse CLI output: {e}\nRaw: {}",
                    truncate(&output_json, 200)
                )
            }
        };

        if markets.is_empty() {
            return "No active markets found on Polymarket.".into();
        }

        let mut output = format!(
            "🔮 **Polymarket Trending** ({} market{}){}\n\n",
            markets.len(),
            if markets.len() == 1 { "" } else { "s" },
            tag.map(|t| format!(" [tag: {t}]")).unwrap_or_default()
        );

        for (i, market) in markets.iter().enumerate() {
            output.push_str(&format_gamma_market(i + 1, market));
        }

        output.push_str("\n🔗 [Polymarket](https://polymarket.com)");
        output
    }
}

// ── PolymarketSearchTool ───────────────────────────────────────────

/// Searches Polymarket for markets matching a query string.
#[derive(Clone)]
pub struct PolymarketSearchTool {
    pub config: PolymarketConfig,
}

impl PolymarketSearchTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketSearchTool {
    fn name(&self) -> &str {
        "polymarket_search"
    }

    fn description(&self) -> &str {
        "Search Polymarket prediction markets by keyword or topic. \
         Returns matching markets with outcomes and current status. \
         Use this when the user asks about specific predictions by topic \
         (e.g. 'Bitcoin', 'elections', 'AI', 'sports')."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (e.g. 'Bitcoin', 'Trump', 'AI regulation')"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return "Error: 'query' parameter is required".into();
        };

        debug!(query, "Searching Polymarket");

        let cli_args = vec![
            "markets", "search", query, "--limit", "10", "--output", "json",
        ];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Search failed via CLI: {e}"),
        };

        let markets: Vec<CustomGammaMarket> = match serde_json::from_str(&output_json) {
            Ok(m) => m,
            Err(e) => {
                return format!(
                    "❌ Failed to parse search results: {e}\nRaw: {}",
                    truncate(&output_json, 200)
                )
            }
        };

        if markets.is_empty() {
            return format!("No markets found matching \"{query}\".");
        }

        let display_markets = &markets[..markets.len().min(10)];

        let mut output = format!(
            "🔍 **Polymarket Search**: \"{query}\" ({} result{})\n\n",
            display_markets.len(),
            if display_markets.len() == 1 { "" } else { "s" }
        );

        for (i, market) in display_markets.iter().enumerate() {
            output.push_str(&format_gamma_market(i + 1, market));
        }

        output
    }
}

// ── PolymarketMarketTool ───────────────────────────────────────────

/// Fetches detailed information for a specific market by condition ID or slug.
#[derive(Clone)]
pub struct PolymarketMarketTool {
    pub config: PolymarketConfig,
}

impl PolymarketMarketTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketMarketTool {
    fn name(&self) -> &str {
        "polymarket_market"
    }

    fn description(&self) -> &str {
        "Get detailed information about a specific Polymarket prediction market \
         by its condition ID or slug. Returns the question, outcomes with live prices, \
         order book depth, and market status. Use this when the user provides \
         a specific market identifier."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "market_id": {
                    "type": "string",
                    "description": "Market condition ID (hex string) or slug (e.g. 'will-trump-win')"
                }
            },
            "required": ["market_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(market_id) = args.get("market_id").and_then(|v| v.as_str()) else {
            return "Error: 'market_id' parameter is required".into();
        };

        debug!(market_id, "Looking up Polymarket market");

        let cli_args = vec!["markets", "get", market_id, "--output", "json"];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Market lookup failed via CLI: {e}"),
        };

        // If it's a slug, it returns a Vector of one market from Gamma.
        // If it's a condition ID, it returns a single Market object from CLOB (simplified).
        // Actually, the CLI handles the routing.

        if let Ok(markets) = serde_json::from_str::<Vec<CustomGammaMarket>>(&output_json) {
            if let Some(m) = markets.first() {
                return format_gamma_market(1, m);
            }
        }

        if let Ok(market) = serde_json::from_str::<ClobSimplifiedMarket>(&output_json) {
            return format_market_detail(&market, &[]);
        }

        format!(
            "❌ Failed to recognize market data format from CLI output.\nRaw: {}",
            truncate(&output_json, 250)
        )
    }
}

// ── Formatting Helpers ─────────────────────────────────────────────

/// Format a Gamma market as a compact summary line for listing.
pub fn format_gamma_market(rank: usize, market: &CustomGammaMarket) -> String {
    let question = if market.question.is_empty() {
        "(untitled)"
    } else {
        &market.question
    };

    let status_icon = if market.closed {
        "🔒"
    } else if market.active {
        "🟢"
    } else {
        "⏸️"
    };

    let outcomes = if market.outcomes.is_empty() {
        "N/A".to_string()
    } else {
        market.outcomes.join(" / ")
    };

    let display_slug = market
        .events
        .first()
        .map(|e| e.slug.clone())
        .unwrap_or_else(|| market.slug.clone());

    format!(
        "{rank}. {status} **{question}**\n   \
         🎯 Outcomes: {outcomes}\n   \
         🔗 [https://polymarket.com/event/{slug}](https://polymarket.com/event/{slug})\n\n",
        rank = rank,
        status = status_icon,
        question = truncate(question, 80),
        outcomes = outcomes,
        slug = display_slug,
    )
}

/// Format a CLOB market with full detail.
fn format_market_detail(
    market: &ClobSimplifiedMarket,
    order_books: &[(String, ClobOrderBook)],
) -> String {
    let question = if market.question.is_empty() {
        "(untitled)"
    } else {
        &market.question
    };

    let status = if market.closed {
        "🔒 Closed"
    } else if market.active {
        "🟢 Active"
    } else {
        "⏸️ Inactive"
    };

    let end_date = market.end_date_iso.as_deref().unwrap_or("No end date");

    let mut tokens_str = String::new();
    for token in &market.tokens {
        let price_pct = token
            .price
            .map(|p| format!("{:.1}%", p * 100.0))
            .unwrap_or_else(|| "N/A".to_string());

        let icon = token
            .price
            .map(|p| if p >= 0.5 { "🟢" } else { "🔴" })
            .unwrap_or("⚪");

        let winner_tag = if token.winner { " 🏆" } else { "" };

        tokens_str.push_str(&format!(
            "  {icon} **{outcome}**: {price}{winner}\n",
            icon = icon,
            outcome = token.outcome,
            price = price_pct,
            winner = winner_tag,
        ));
    }

    let mut book_str = String::new();
    for (outcome, book) in order_books {
        let best_bid = book
            .bids
            .first()
            .map(|e| format!("${}", e.price))
            .unwrap_or_else(|| "—".into());
        let best_ask = book
            .asks
            .first()
            .map(|e| format!("${}", e.price))
            .unwrap_or_else(|| "—".into());
        let bid_depth = book.bids.len();
        let ask_depth = book.asks.len();

        book_str.push_str(&format!(
            "  📘 **{outcome}**: Bid {best_bid} ({bid_depth} lvls) \
             | Ask {best_ask} ({ask_depth} lvls)\n",
        ));
    }

    let book_section = if book_str.is_empty() {
        String::new()
    } else {
        format!("\n📊 **Order Book**\n{book_str}")
    };

    format!(
        "🔮 **{question}**\n\n\
         Status: {status}\n\
         Ends: 📅 {end_date}\n\n\
         📊 **Outcomes & Odds**\n\
         {tokens}\
         {book}\
         \n🪪 Condition: `{cid}`",
        question = question,
        status = status,
        end_date = end_date,
        tokens = tokens_str,
        book = book_section,
        cid = market.condition_id,
    )
}

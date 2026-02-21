//! Polymarket prediction-market tools.
//!
//! Provides real-time access to Polymarket prediction markets via the
//! official [`polymarket_sdk`] crate (`polymarket-rust-sdk`). Gives CrabbyBot
//! a unique competitive advantage — no other lightweight AI agent has native
//! prediction-market intelligence.
//!
//! ## Architecture
//!
//! All tools share a common pattern:
//! 1. Parse user arguments from JSON
//! 2. Create a `PolymarketClient<Public>` (no auth needed for reads)
//! 3. Call the appropriate SDK method
//! 4. Format the response into a rich, human-readable string
//!
//! ## Usage in Telegram
//!
//! Just ask naturally:
//! - "What are the trending prediction markets?"
//! - "Search Polymarket for Bitcoin"
//! - "Get details on Polymarket condition 0x123..."

use async_trait::async_trait;
use polymarket_sdk::{GammaMarket, OrderBook, PolymarketClient, SimplifiedMarket};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::Tool;

// ── PolymarketTrendingTool ─────────────────────────────────────────

/// Fetches the most active prediction markets from Polymarket's Gamma API.
pub struct PolymarketTrendingTool;

impl PolymarketTrendingTool {
    pub fn new() -> Self {
        Self
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
         Use this when the user asks about trending predictions or hot markets."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "number",
                    "description": "Number of trending markets to return (default: 5, max: 10)"
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
            .min(10) as u32;

        debug!(limit, "Fetching trending Polymarket markets");

        let client = match PolymarketClient::new_public(None) {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to initialize Polymarket client: {e}"),
        };

        let markets = match client.get_gamma_markets(Some(limit), None).await {
            Ok(m) => m,
            Err(e) => return format!("❌ Failed to fetch trending markets: {e}"),
        };

        if markets.is_empty() {
            return "No active markets found on Polymarket.".into();
        }

        let mut output = format!(
            "🔮 **Polymarket Trending** ({} market{})\n\n",
            markets.len(),
            if markets.len() == 1 { "" } else { "s" }
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
pub struct PolymarketSearchTool;

impl PolymarketSearchTool {
    pub fn new() -> Self {
        Self
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

        let client = match PolymarketClient::new_public(None) {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to initialize Polymarket client: {e}"),
        };

        let markets = match client.search_markets(query).await {
            Ok(m) => m,
            Err(e) => return format!("❌ Search failed: {e}"),
        };

        if markets.is_empty() {
            return format!("No markets found matching \"{query}\".");
        }

        // Limit to 10 results max for readability
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

/// Fetches detailed information for a specific market by condition ID,
/// including the live order book.
pub struct PolymarketMarketTool;

impl PolymarketMarketTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketMarketTool {
    fn name(&self) -> &str {
        "polymarket_market"
    }

    fn description(&self) -> &str {
        "Get detailed information about a specific Polymarket prediction market \
         by its condition ID. Returns the question, outcomes with live prices, \
         order book depth, and market status. Use this when the user provides \
         a specific market or condition ID."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "condition_id": {
                    "type": "string",
                    "description": "The condition ID (hex string) of the market to look up"
                }
            },
            "required": ["condition_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(condition_id) = args.get("condition_id").and_then(|v| v.as_str()) else {
            return "Error: 'condition_id' parameter is required".into();
        };

        debug!(condition_id, "Looking up Polymarket market");

        let client = match PolymarketClient::new_public(None) {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to initialize Polymarket client: {e}"),
        };

        // Fetch the market info from the CLOB API
        let market = match client.get_market(condition_id).await {
            Ok(m) => m,
            Err(e) => return format!("❌ Failed to fetch market: {e}"),
        };

        // Try to fetch order books for each token
        let mut order_books = Vec::new();
        for token in &market.tokens {
            match client.get_order_book(&token.token_id).await {
                Ok(book) => order_books.push((token.outcome.clone(), book)),
                Err(_) => {} // silently skip if order book unavailable
            }
        }

        format_market_detail(&market, &order_books)
    }
}

// ── Formatting Helpers ─────────────────────────────────────────────

/// Format a Gamma market as a compact summary line for listing.
fn format_gamma_market(rank: usize, market: &GammaMarket) -> String {
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

    format!(
        "{rank}. {status} **{question}**\n   \
         🎯 Outcomes: {outcomes}\n   \
         🔗 [polymarket.com/{slug}](https://polymarket.com/event/{slug})\n\n",
        rank = rank,
        status = status_icon,
        question = truncate(question, 80),
        outcomes = outcomes,
        slug = market.slug,
    )
}

/// Format a simplified market with full detail for single-market lookups.
fn format_market_detail(market: &SimplifiedMarket, order_books: &[(String, OrderBook)]) -> String {
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

    let end_date = market
        .end_date_iso
        .as_deref()
        .unwrap_or("No end date");

    // Format token outcomes with live prices
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

    // Format order book summaries
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

/// Truncate a string to `max_len` characters, appending "…" if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

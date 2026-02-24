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
use tracing::{debug, error};

use super::polymarket_common::{build_http_client, truncate, CLOB_API_URL, GAMMA_API_URL};
use super::Tool;

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
#[derive(Default)]
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

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        let mut url = format!(
            "{}/markets?limit={}&active=true&closed=false&order=volume24hr&ascending=false",
            GAMMA_API_URL, limit
        );
        if let Some(t) = tag {
            url.push_str(&format!("&tag={}", t));
        }

        let markets: Vec<CustomGammaMarket> = match client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    error!(%status, %body, "Gamma API error");
                    return format!("❌ Gamma API returned {status}: {}", truncate(&body, 200));
                }
                match resp.json().await {
                    Ok(m) => m,
                    Err(e) => return format!("❌ Failed to parse market data: {e}"),
                }
            }
            Err(e) => return format!("❌ Failed to reach Polymarket: {e}"),
        };

        if markets.is_empty() {
            return "No active markets found on Polymarket.".into();
        }

        let mut output = format!(
            "🔮 **Polymarket Trending** ({} market{}){}\n\n",
            markets.len(),
            if markets.len() == 1 { "" } else { "s" },
            tag.map(|t| format!(" [tag: {t}]"))
                .unwrap_or_default()
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
#[derive(Default)]
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

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        let url = format!("{}/markets", GAMMA_API_URL);
        let markets: Vec<CustomGammaMarket> = match client
            .get(&url)
            .query(&[
                ("_q", query),
                ("active", "true"),
                ("closed", "false"),
                ("order", "volume24hr"),
                ("ascending", "false"),
            ])
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    error!(%status, %body, "Gamma API search error");
                    return format!("❌ Search failed ({status}): {}", truncate(&body, 200));
                }
                match resp.json().await {
                    Ok(m) => m,
                    Err(e) => return format!("❌ Failed to parse search results: {e}"),
                }
            }
            Err(e) => return format!("❌ Failed to reach Polymarket: {e}"),
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
#[derive(Default)]
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

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        // Determine if this is a slug or condition ID
        let is_slug = !market_id.starts_with("0x") && market_id.contains('-');

        if is_slug {
            // Fetch from Gamma API by slug
            let url = format!("{}/markets?slug={}", GAMMA_API_URL, market_id);
            let markets: Vec<CustomGammaMarket> = match client.get(&url).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        return format!("❌ Slug lookup failed ({status}): {}", truncate(&body, 200));
                    }
                    match resp.json().await {
                        Ok(m) => m,
                        Err(e) => return format!("❌ Failed to parse market: {e}"),
                    }
                }
                Err(e) => return format!("❌ Failed to reach Polymarket: {e}"),
            };

            match markets.first() {
                Some(m) => format_gamma_market(1, m),
                None => format!("No market found with slug \"{market_id}\"."),
            }
        } else {
            // Fetch from CLOB API by condition ID
            let url = format!("{}/markets/{}", CLOB_API_URL, market_id);
            let market: ClobSimplifiedMarket = match client.get(&url).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        error!(%status, %body, "CLOB market fetch error");
                        return format!(
                            "❌ Market lookup failed ({status}): {}",
                            truncate(&body, 200)
                        );
                    }
                    match resp.json().await {
                        Ok(m) => m,
                        Err(e) => return format!("❌ Failed to parse market data: {e}"),
                    }
                }
                Err(e) => return format!("❌ Failed to reach Polymarket: {e}"),
            };

            // Fetch order books for each token
            let mut order_books = Vec::new();
            for token in &market.tokens {
                let book_url = format!("{}/book", CLOB_API_URL);
                if let Ok(resp) = client
                    .get(&book_url)
                    .query(&[("token_id", &token.token_id)])
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        if let Ok(book) = resp.json::<ClobOrderBook>().await {
                            order_books.push((token.outcome.clone(), book));
                        }
                    }
                }
            }

            format_market_detail(&market, &order_books)
        }
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

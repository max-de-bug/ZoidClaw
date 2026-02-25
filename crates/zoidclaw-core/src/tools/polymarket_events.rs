//! Polymarket Events tools.
//!
//! Events group related markets (e.g. "2024 Election" contains
//! multiple yes/no markets). Provides listing with filters and
//! detail lookup by ID or slug.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, error};

use super::polymarket_common::{run_polymarket_cli, truncate};
use super::Tool;
use crate::config::PolymarketConfig;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEvent {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    markets: Vec<GammaEventMarket>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEventMarket {
    #[serde(default)]
    question: String,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    outcome_prices: Option<String>,
}

// ── PolymarketEventsTool ───────────────────────────────────────────

/// List/browse Polymarket events with optional filters.
#[derive(Clone)]
pub struct PolymarketEventsTool {
    pub config: PolymarketConfig,
}

impl PolymarketEventsTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketEventsTool {
    fn name(&self) -> &str {
        "polymarket_events"
    }

    fn description(&self) -> &str {
        "List Polymarket events (groups of related prediction markets). \
         Filter by tag (politics, crypto, sports), active/closed status, \
         and sort order. Use this when asking about event categories or \
         groups of related markets."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tag": {
                    "type": "string",
                    "description": "Filter by tag slug (e.g. 'politics', 'crypto', 'sports')"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results to return (default: 5, max: 15)"
                },
                "active": {
                    "type": "boolean",
                    "description": "Filter to active events only (default: true)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(15);
        let tag = args.get("tag").and_then(|v| v.as_str());
        let active = args.get("active").and_then(|v| v.as_bool()).unwrap_or(true);

        debug!(?tag, limit, active, "Listing Polymarket events");

        let limit_str = limit.to_string();
        let mut cli_args = vec!["events", "list", "--limit", &limit_str];
        if let Some(t) = tag {
            cli_args.extend_from_slice(&["--tag", t]);
        }
        if active {
            cli_args.push("--active");
        }
        cli_args.extend_from_slice(&["--output", "json"]);

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Failed to fetch events via CLI: {e}"),
        };

        let events: Vec<GammaEvent> = match serde_json::from_str(&output_json) {
            Ok(e) => e,
            Err(e) => {
                return format!(
                    "❌ Failed to parse events: {e}\nRaw: {}",
                    truncate(&output_json, 200)
                )
            }
        };

        if events.is_empty() {
            return "No events found matching the criteria.".into();
        }

        let mut output = format!(
            "📋 **Polymarket Events** ({} event{}){}\n\n",
            events.len(),
            if events.len() == 1 { "" } else { "s" },
            tag.map(|t| format!(" [tag: {t}]")).unwrap_or_default()
        );

        for (i, event) in events.iter().enumerate() {
            output.push_str(&format_event_summary(i + 1, event));
        }

        output
    }
}

// ── PolymarketEventDetailTool ──────────────────────────────────────

/// Get detailed info for a single event by ID or slug.
#[derive(Clone)]
pub struct PolymarketEventDetailTool {
    pub config: PolymarketConfig,
}

impl PolymarketEventDetailTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketEventDetailTool {
    fn name(&self) -> &str {
        "polymarket_event_detail"
    }

    fn description(&self) -> &str {
        "Get detailed information about a specific Polymarket event by its \
         numeric ID or slug. Returns the event description with all child \
         markets and their current odds."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "Event numeric ID or slug string"
                }
            },
            "required": ["event_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(event_id) = args.get("event_id").and_then(|v| v.as_str()) else {
            return "Error: 'event_id' parameter is required".into();
        };

        debug!(event_id, "Fetching Polymarket event detail");

        let cli_args = vec!["events", "get", event_id, "--output", "json"];

        let output_json = match run_polymarket_cli(&self.config, &cli_args).await {
            Ok(out) => out,
            Err(e) => return format!("❌ Event lookup failed via CLI: {e}"),
        };

        let event: GammaEvent = match serde_json::from_str(&output_json) {
            Ok(e) => e,
            Err(_) => {
                // Could be that it returned an array of one event if searched by slug
                if let Ok(events) = serde_json::from_str::<Vec<GammaEvent>>(&output_json) {
                    if let Some(e) = events.into_iter().next() {
                        e
                    } else {
                        return format!("No event found with slug \"{event_id}\".");
                    }
                } else {
                    return format!(
                        "❌ Failed to parse event details from CLI.\nRaw: {}",
                        truncate(&output_json, 250)
                    );
                }
            }
        };

        format_event_detail(&event)
    }
}

// ── Formatting ─────────────────────────────────────────────────────

fn format_event_summary(rank: usize, event: &GammaEvent) -> String {
    let title = event.title.as_deref().unwrap_or("(untitled)");
    let status = if event.closed {
        "🔒"
    } else if event.active {
        "🟢"
    } else {
        "⏸️"
    };
    let market_count = event.markets.len();

    format!(
        "{rank}. {status} **{title}**\n   \
         📊 {market_count} market{s}\n   \
         🔗 [https://polymarket.com/event/{slug}](https://polymarket.com/event/{slug})\n\n",
        rank = rank,
        status = status,
        title = truncate(title, 80),
        market_count = market_count,
        s = if market_count == 1 { "" } else { "s" },
        slug = event.slug,
    )
}

fn format_event_detail(event: &GammaEvent) -> String {
    let title = event.title.as_deref().unwrap_or("(untitled)");
    let status = if event.closed {
        "🔒 Closed"
    } else if event.active {
        "🟢 Active"
    } else {
        "⏸️ Inactive"
    };

    let description = event
        .description
        .as_deref()
        .map(|d| truncate(d, 300))
        .unwrap_or_default();

    let mut markets_str = String::new();
    for (i, m) in event.markets.iter().enumerate() {
        let m_status = if m.closed {
            "🔒"
        } else if m.active {
            "🟢"
        } else {
            "⏸️"
        };

        // Parse outcome prices if available
        let prices = m.outcome_prices.as_deref().unwrap_or("");
        let price_display = if !prices.is_empty() {
            // Prices are typically "[\"0.52\",\"0.48\"]"
            if let Ok(p) = serde_json::from_str::<Vec<String>>(prices) {
                p.iter()
                    .enumerate()
                    .map(|(j, pv)| {
                        let label = if j == 0 { "Yes" } else { "No" };
                        if let Ok(f) = pv.parse::<f64>() {
                            format!("{label}: {:.1}%", f * 100.0)
                        } else {
                            format!("{label}: {pv}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            } else {
                prices.to_string()
            }
        } else {
            "N/A".to_string()
        };

        markets_str.push_str(&format!(
            "  {}. {m_status} {question}\n     📊 {prices}\n",
            i + 1,
            m_status = m_status,
            question = truncate(&m.question, 70),
            prices = price_display,
        ));
    }

    let market_count = event.markets.len();

    format!(
        "📋 **{title}**\n\n\
         Status: {status}\n\
         {desc}\n\n\
         📊 **Markets** ({count} market{s})\n\
         {markets}\n\
         🔗 [https://polymarket.com/event/{slug}](https://polymarket.com/event/{slug})",
        title = title,
        status = status,
        desc = if description.is_empty() {
            String::new()
        } else {
            format!("📝 {description}\n")
        },
        count = market_count,
        s = if market_count == 1 { "" } else { "s" },
        markets = markets_str,
        slug = event.slug,
    )
}

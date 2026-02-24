//! Polymarket sports tools.
//!
//! Browse supported sports, market types, and teams. Read-only.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, truncate, GAMMA_API_URL};
use super::Tool;

// ── PolymarketSportsTool ───────────────────────────────────────────

/// Browse Polymarket sports metadata and teams.
#[derive(Default)]
pub struct PolymarketSportsTool;

impl PolymarketSportsTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketSportsTool {
    fn name(&self) -> &str { "polymarket_sports" }

    fn description(&self) -> &str {
        "Browse Polymarket sports metadata. Use 'list' to see supported sports, \
         'types' for market types, or 'teams' to browse teams by league."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "types", "teams"],
                    "description": "Action: 'list' sports, 'types' for market types, 'teams' to browse teams"
                },
                "league": {
                    "type": "string",
                    "description": "Filter teams by league (optional, for teams action)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results for teams (default: 20)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let league = args.get("league").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).min(50);

        debug!(action, ?league, "Polymarket sports");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        match action {
            "list" => {
                let url = format!("{}/sports", GAMMA_API_URL);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("⚽ **Supported Sports**\n\n{}", truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "types" => {
                let url = format!("{}/sports/market-types", GAMMA_API_URL);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("📋 **Sports Market Types**\n\n{}", truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "teams" => {
                let mut url = format!("{}/sports/teams?limit={}", GAMMA_API_URL, limit);
                if let Some(lg) = league {
                    url.push_str(&format!("&league={}", lg));
                }
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("🏟️ **Teams**{}\n\n{}", 
                            league.map(|l| format!(" ({l})")).unwrap_or_default(),
                            truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            _ => format!("Error: unknown action '{action}'. Use 'list', 'types', or 'teams'."),
        }
    }
}

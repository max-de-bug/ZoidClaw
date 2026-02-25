//! Polymarket series tools.
//!
//! Browse and look up market series. Read-only — no wallet needed.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, truncate, GAMMA_API_URL};
use super::Tool;

// ── PolymarketSeriesTool ───────────────────────────────────────────

/// Browse and look up Polymarket series.
#[derive(Default)]
pub struct PolymarketSeriesTool;

impl PolymarketSeriesTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketSeriesTool {
    fn name(&self) -> &str {
        "polymarket_series"
    }

    fn description(&self) -> &str {
        "Browse Polymarket series (groups of recurring events). \
         Use 'list' to browse series or 'get' to view a specific series by ID."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get"],
                    "description": "Action: 'list' all series or 'get' one by ID"
                },
                "id": {
                    "type": "string",
                    "description": "Series ID (required for get)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results for list (default: 10)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let id = args.get("id").and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(25);

        debug!(action, ?id, "Polymarket series");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        match action {
            "list" => {
                let url = format!(
                    "{}/series?limit={}&order=volume&ascending=false",
                    GAMMA_API_URL, limit
                );
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("📚 **Polymarket Series**\n\n{}", truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "get" => {
                let Some(series_id) = id else {
                    return "Error: 'id' is required for get action".into();
                };
                let url = format!("{}/series/{}", GAMMA_API_URL, series_id);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("📚 **Series Detail**\n\n{}", truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            _ => format!("Error: unknown action '{action}'. Use 'list' or 'get'."),
        }
    }
}

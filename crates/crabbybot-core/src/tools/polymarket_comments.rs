//! Polymarket comments tools.
//!
//! Read comments on events, markets, and series. Read-only — no wallet needed.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, truncate, GAMMA_API_URL};
use super::Tool;

// ── PolymarketCommentsTool ─────────────────────────────────────────

/// Read Polymarket comments on events, markets, or series.
#[derive(Default)]
pub struct PolymarketCommentsTool;

impl PolymarketCommentsTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketCommentsTool {
    fn name(&self) -> &str { "polymarket_comments" }

    fn description(&self) -> &str {
        "Read comments on Polymarket events, markets, or series. \
         Can also look up comments by a specific user address."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "by_user"],
                    "description": "Action: 'list' comments on an entity, 'get' by ID, or 'by_user' for user comments"
                },
                "entity_type": {
                    "type": "string",
                    "enum": ["event", "market", "series"],
                    "description": "Entity type (required for list)"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity ID (required for list) or comment ID (for get) or user address (for by_user)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results (default: 10)"
                }
            },
            "required": ["action", "entity_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let entity_id = args.get("entity_id").and_then(|v| v.as_str()).unwrap_or("");
        let entity_type = args.get("entity_type").and_then(|v| v.as_str()).unwrap_or("event");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25);

        debug!(action, entity_type, entity_id, "Polymarket comments");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        match action {
            "list" => {
                let url = format!(
                    "{}/comments?parentEntityType={}&parentEntityId={}&limit={}",
                    GAMMA_API_URL, entity_type, entity_id, limit
                );
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("💬 **Comments** on {entity_type} `{}`\n\n{}", truncate(entity_id, 20), truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "get" => {
                let url = format!("{}/comments/{}", GAMMA_API_URL, entity_id);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("💬 **Comment Detail**\n\n{}", truncate(&body, 500))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "by_user" => {
                let url = format!("{}/comments?userAddress={}&limit={}", GAMMA_API_URL, entity_id, limit);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("💬 **Comments by** `{}`\n\n{}", truncate(entity_id, 20), truncate(&body, 1000))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            _ => format!("Error: unknown action '{action}'. Use 'list', 'get', or 'by_user'."),
        }
    }
}

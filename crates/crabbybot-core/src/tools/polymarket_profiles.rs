//! Polymarket profile tools.
//!
//! Look up public profiles by wallet address. Read-only.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, truncate, GAMMA_API_URL};
use super::Tool;

// ── PolymarketProfileTool ──────────────────────────────────────────

/// Look up a Polymarket public profile by wallet address.
#[derive(Default)]
pub struct PolymarketProfileTool;

impl PolymarketProfileTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketProfileTool {
    fn name(&self) -> &str { "polymarket_profile" }

    fn description(&self) -> &str {
        "Look up a Polymarket user's public profile by wallet address. \
         Shows display name, bio, and trading stats."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Wallet address (0x...)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' is required".into();
        };
        debug!(address, "Fetching Polymarket profile");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let url = format!("{}/profiles/{}", GAMMA_API_URL, address);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                format!("👤 **Profile** for `{}`\n\n{}", truncate(address, 20), truncate(&body, 1000))
            }
            Ok(resp) => format!("❌ API error ({})", resp.status()),
            Err(e) => format!("❌ Request failed: {e}"),
        }
    }
}

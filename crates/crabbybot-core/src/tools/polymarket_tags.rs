//! Polymarket tags tools.
//!
//! Browse and look up market tags, get related tags.
//! All read-only — no wallet needed.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, truncate, GAMMA_API_URL};
use super::Tool;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GammaTag {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    slug: Option<String>,
}

// ── PolymarketTagsTool ─────────────────────────────────────────────

/// Browse and look up Polymarket tags.
#[derive(Default)]
pub struct PolymarketTagsTool;

impl PolymarketTagsTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for PolymarketTagsTool {
    fn name(&self) -> &str { "polymarket_tags" }

    fn description(&self) -> &str {
        "Browse Polymarket tags (categories like politics, crypto, sports). \
         Use 'list' to browse all tags, 'get' to view a specific tag, or \
         'related' to see tags related to a given tag."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "related"],
                    "description": "Action: list all tags, get a tag by ID/slug, or find related tags"
                },
                "id": {
                    "type": "string",
                    "description": "Tag ID or slug (required for get/related)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results for list (default: 20)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let id = args.get("id").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).min(50);

        debug!(action, ?id, "Polymarket tags");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        match action {
            "list" => {
                let url = format!("{}/tags?limit={}", GAMMA_API_URL, limit);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let tags: Vec<GammaTag> = match resp.json().await {
                            Ok(t) => t,
                            Err(e) => return format!("❌ Parse error: {e}"),
                        };
                        if tags.is_empty() { return "No tags found.".into(); }
                        let mut out = format!("🏷️ **Polymarket Tags** ({} tags)\n\n", tags.len());
                        for tag in &tags {
                            let label = tag.label.as_deref().unwrap_or("?");
                            let slug = tag.slug.as_deref().unwrap_or("?");
                            out.push_str(&format!("• **{label}** (`{slug}`)\n"));
                        }
                        out
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "get" => {
                let Some(tag_id) = id else {
                    return "Error: 'id' is required for get action".into();
                };
                let is_numeric = tag_id.chars().all(|c| c.is_ascii_digit());
                let url = if is_numeric {
                    format!("{}/tags/{}", GAMMA_API_URL, tag_id)
                } else {
                    format!("{}/tags/slug/{}", GAMMA_API_URL, tag_id)
                };
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        format!("🏷️ **Tag Detail**\n\n{}", truncate(&body, 500))
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            "related" => {
                let Some(tag_id) = id else {
                    return "Error: 'id' is required for related action".into();
                };
                let is_numeric = tag_id.chars().all(|c| c.is_ascii_digit());
                let url = if is_numeric {
                    format!("{}/tags/{}/related", GAMMA_API_URL, tag_id)
                } else {
                    format!("{}/tags/slug/{}/related", GAMMA_API_URL, tag_id)
                };
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        match serde_json::from_str::<Vec<GammaTag>>(&body) {
                            Ok(tags) => {
                                let mut out = format!("🏷️ **Related Tags** for `{tag_id}`\n\n");
                                for tag in &tags {
                                    let label = tag.label.as_deref().unwrap_or("?");
                                    out.push_str(&format!("• {label}\n"));
                                }
                                out
                            }
                            Err(_) => {
                                format!("🏷️ **Related Tags**\n\n{}", truncate(&body, 500))
                            }
                        }
                    }
                    Ok(resp) => format!("❌ API error ({})", resp.status()),
                    Err(e) => format!("❌ Request failed: {e}"),
                }
            }
            _ => format!("Error: unknown action '{action}'. Use 'list', 'get', or 'related'."),
        }
    }
}

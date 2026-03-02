//! Polymarket API status / health check tool.
//!
//! Quick check that the CLOB and Gamma APIs are reachable.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, CLOB_API_URL, GAMMA_API_URL};
use super::Tool;

// ── PolymarketStatusTool ───────────────────────────────────────────

/// Check Polymarket API health status.
#[derive(Default)]
pub struct PolymarketStatusTool;

impl PolymarketStatusTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketStatusTool {
    fn name(&self) -> &str {
        "polymarket_status"
    }

    fn description(&self) -> &str {
        "Check the health status of Polymarket APIs (CLOB and Gamma). \
         Use this to diagnose connectivity issues."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: HashMap<String, Value>) -> String {
        debug!("Checking Polymarket API status");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ HTTP client error: {e}"),
        };

        let clob_url = format!("{}/", CLOB_API_URL);
        let gamma_url = format!("{}/markets?limit=1", GAMMA_API_URL);

        let (clob_res, gamma_res) =
            tokio::join!(client.get(&clob_url).send(), client.get(&gamma_url).send(),);

        let clob_status = match clob_res {
            Ok(r) if r.status().is_success() => "🟢 OK".to_string(),
            Ok(r) => format!("🟡 {}", r.status()),
            Err(e) => format!("🔴 Down ({e})"),
        };

        let gamma_status = match gamma_res {
            Ok(r) if r.status().is_success() => "🟢 OK".to_string(),
            Ok(r) => format!("🟡 {}", r.status()),
            Err(e) => format!("🔴 Down ({e})"),
        };

        format!(
            "🏥 **Polymarket API Status**\n\n\
             📊 CLOB API: {clob_status}\n\
             🔍 Gamma API: {gamma_status}"
        )
    }
}

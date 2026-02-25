//! Polymarket bridge tools.
//!
//! Get deposit addresses for Polymarket from any supported chain
//! (EVM, Solana, Bitcoin), check supported assets, and verify
//! deposit status.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::{build_http_client, truncate};
use super::Tool;

const BRIDGE_API_URL: &str = "https://bridge-api.polymarket.com";

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DepositResponse {
    #[serde(default)]
    evm_address: Option<String>,
    #[serde(default)]
    solana_address: Option<String>,
    #[serde(default)]
    bitcoin_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupportedAsset {
    #[serde(default)]
    chain: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupportedAssetsResponse {
    #[serde(default)]
    assets: Vec<SupportedAsset>,
}

// ── PolymarketBridgeTool ───────────────────────────────────────────

/// Get deposit addresses and check bridge status.
#[derive(Default)]
pub struct PolymarketBridgeTool;

impl PolymarketBridgeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketBridgeTool {
    fn name(&self) -> &str {
        "polymarket_bridge"
    }

    fn description(&self) -> &str {
        "Polymarket bridge operations: get deposit addresses from any \
         supported chain (EVM, Solana, Bitcoin), list supported assets, \
         or check deposit status. No wallet needed for querying."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["deposit", "supported_assets", "status"],
                    "description": "Action: 'deposit' to get addresses, 'supported_assets' to list chains/tokens, 'status' to check deposit"
                },
                "address": {
                    "type": "string",
                    "description": "Wallet address for deposit/status queries (0x... for EVM)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(action) = args.get("action").and_then(|v| v.as_str()) else {
            return "Error: 'action' is required".into();
        };
        let address = args.get("address").and_then(|v| v.as_str());

        debug!(action, ?address, "Polymarket bridge operation");

        let client = match build_http_client() {
            Ok(c) => c,
            Err(e) => return format!("❌ Failed to create HTTP client: {e}"),
        };

        match action {
            "deposit" => {
                let Some(addr) = address else {
                    return "Error: 'address' is required for deposit lookup".into();
                };

                let url = format!("{}/deposit", BRIDGE_API_URL);
                match client.get(&url).query(&[("address", addr)]).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            return format!(
                                "❌ Bridge API error ({status}): {}",
                                truncate(&body, 200)
                            );
                        }
                        match resp.json::<DepositResponse>().await {
                            Ok(dep) => {
                                let evm = dep.evm_address.as_deref().unwrap_or("N/A");
                                let sol = dep.solana_address.as_deref().unwrap_or("N/A");
                                let btc = dep.bitcoin_address.as_deref().unwrap_or("N/A");

                                format!(
                                    "🌉 **Deposit Addresses** for `{addr}`\n\n\
                                     🔷 **EVM**: `{evm}`\n\
                                     ◎ **Solana**: `{sol}`\n\
                                     ₿ **Bitcoin**: `{btc}`\n\n\
                                     Send assets to these addresses to deposit into Polymarket.",
                                    addr = truncate(addr, 20),
                                    evm = evm,
                                    sol = sol,
                                    btc = btc,
                                )
                            }
                            Err(e) => format!("❌ Failed to parse deposit response: {e}"),
                        }
                    }
                    Err(e) => format!("❌ Failed to reach Bridge API: {e}"),
                }
            }
            "supported_assets" => {
                let url = format!("{}/supported-assets", BRIDGE_API_URL);
                match client.get(&url).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            return format!(
                                "❌ Bridge API error ({status}): {}",
                                truncate(&body, 200)
                            );
                        }
                        match resp.json::<SupportedAssetsResponse>().await {
                            Ok(assets) => {
                                if assets.assets.is_empty() {
                                    return "No supported assets found.".into();
                                }
                                let mut output = "🌉 **Supported Bridge Assets**\n\n".to_string();
                                for asset in &assets.assets {
                                    let chain = asset.chain.as_deref().unwrap_or("?");
                                    let symbol = asset.symbol.as_deref().unwrap_or("?");
                                    output.push_str(&format!("• **{symbol}** on {chain}\n",));
                                }
                                output
                            }
                            Err(e) => {
                                format!("❌ Failed to parse supported assets: {e}")
                            }
                        }
                    }
                    Err(e) => format!("❌ Failed to reach Bridge API: {e}"),
                }
            }
            "status" => {
                let Some(addr) = address else {
                    return "Error: 'address' is required for status check".into();
                };

                let url = format!("{}/status", BRIDGE_API_URL);
                match client.get(&url).query(&[("address", addr)]).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            return format!(
                                "❌ Status check error ({status}): {}",
                                truncate(&body, 200)
                            );
                        }
                        let body = resp.text().await.unwrap_or_default();
                        format!(
                            "🌉 **Deposit Status** for `{addr}`\n\n{body}",
                            addr = truncate(addr, 20),
                            body = truncate(&body, 500),
                        )
                    }
                    Err(e) => format!("❌ Failed to reach Bridge API: {e}"),
                }
            }
            _ => format!(
                "Error: unknown action '{action}'. Use 'deposit', 'supported_assets', or 'status'."
            ),
        }
    }
}

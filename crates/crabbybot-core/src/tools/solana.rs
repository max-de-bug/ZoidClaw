//! Solana blockchain tools.
//!
//! Provides on-chain data access via Solana's JSON-RPC API.
//! Makes ferrobot crypto-native with real wallet and token data.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::Tool;

/// Lamports per SOL.
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

// ── SolanaBalanceTool ───────────────────────────────────────────────

pub struct SolanaBalanceTool {
    client: Client,
    rpc_url: String,
}

impl SolanaBalanceTool {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            client: Client::new(),
            rpc_url: rpc_url.to_string(),
        }
    }
}

#[async_trait]
impl Tool for SolanaBalanceTool {
    fn name(&self) -> &str {
        "solana_balance"
    }

    fn description(&self) -> &str {
        "Get the SOL balance of a Solana wallet address. \
         Returns the balance in SOL and lamports."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Solana wallet address (base58 public key)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' parameter is required".into();
        };

        debug!(address, "Fetching Solana balance");

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [address]
        });

        match self.client.post(&self.rpc_url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(data) => {
                        if let Some(err) = data.get("error") {
                            return format!("RPC error: {}", err);
                        }
                        let lamports = data["result"]["value"]
                            .as_u64()
                            .unwrap_or(0);
                        let sol = lamports as f64 / LAMPORTS_PER_SOL;
                        format!(
                            "💰 **Solana Balance**\n\
                             Address: `{}`\n\
                             Balance: **{:.6} SOL** ({} lamports)",
                            address, sol, lamports
                        )
                    }
                    Err(e) => format!("Error parsing response: {}", e),
                }
            }
            Ok(resp) => format!("RPC error (HTTP {})", resp.status()),
            Err(e) => format!("Request failed: {}", e),
        }
    }
}

// ── SolanaTransactionsTool ──────────────────────────────────────────

pub struct SolanaTransactionsTool {
    client: Client,
    rpc_url: String,
}

impl SolanaTransactionsTool {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            client: Client::new(),
            rpc_url: rpc_url.to_string(),
        }
    }
}

#[derive(Deserialize)]
struct SignatureInfo {
    signature: String,
    slot: u64,
    err: Option<Value>,
    #[serde(rename = "blockTime")]
    block_time: Option<i64>,
    memo: Option<String>,
}

#[async_trait]
impl Tool for SolanaTransactionsTool {
    fn name(&self) -> &str {
        "solana_transactions"
    }

    fn description(&self) -> &str {
        "Get recent transaction history for a Solana wallet address. \
         Returns the latest transactions with signatures and timestamps."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Solana wallet address (base58 public key)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Number of transactions to return (default: 10, max: 20)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' parameter is required".into();
        };

        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(20);

        debug!(address, limit, "Fetching Solana transactions");

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignaturesForAddress",
            "params": [
                address,
                { "limit": limit, "commitment": "confirmed" }
            ]
        });

        match self.client.post(&self.rpc_url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(data) => {
                        if let Some(err) = data.get("error") {
                            return format!("RPC error: {}", err);
                        }

                        let sigs: Vec<SignatureInfo> = match serde_json::from_value(
                            data["result"].clone(),
                        ) {
                            Ok(s) => s,
                            Err(e) => return format!("Error parsing transactions: {}", e),
                        };

                        if sigs.is_empty() {
                            return format!("No transactions found for `{}`", address);
                        }

                        let mut output = format!(
                            "📜 **Recent Transactions** for `{}`\n\n",
                            address
                        );

                        for (i, sig) in sigs.iter().enumerate() {
                            let time_str = sig
                                .block_time
                                .map(|t| {
                                    chrono::DateTime::from_timestamp(t, 0)
                                        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                                        .unwrap_or_else(|| t.to_string())
                                })
                                .unwrap_or_else(|| "unknown".into());

                            let status = if sig.err.is_some() { "❌" } else { "✅" };
                            let memo_str = sig
                                .memo
                                .as_deref()
                                .map(|m| format!(" | memo: {}", m))
                                .unwrap_or_default();

                            output.push_str(&format!(
                                "{}. {} `{}...` | slot {} | {}{}\n",
                                i + 1,
                                status,
                                &sig.signature[..16],
                                sig.slot,
                                time_str,
                                memo_str,
                            ));
                        }

                        output
                    }
                    Err(e) => format!("Error parsing response: {}", e),
                }
            }
            Ok(resp) => format!("RPC error (HTTP {})", resp.status()),
            Err(e) => format!("Request failed: {}", e),
        }
    }
}

// ── SolanaTokenBalancesTool ─────────────────────────────────────────

pub struct SolanaTokenBalancesTool {
    client: Client,
    rpc_url: String,
}

impl SolanaTokenBalancesTool {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            client: Client::new(),
            rpc_url: rpc_url.to_string(),
        }
    }
}

#[async_trait]
impl Tool for SolanaTokenBalancesTool {
    fn name(&self) -> &str {
        "solana_token_balances"
    }

    fn description(&self) -> &str {
        "Get all SPL token balances for a Solana wallet (USDC, USDT, etc.). \
         Returns token mint addresses and amounts."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "Solana wallet address (base58 public key)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "Error: 'address' parameter is required".into();
        };

        debug!(address, "Fetching Solana token balances");

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [
                address,
                { "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" },
                { "encoding": "jsonParsed" }
            ]
        });

        match self.client.post(&self.rpc_url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(data) => {
                        if let Some(err) = data.get("error") {
                            return format!("RPC error: {}", err);
                        }

                        let accounts = data["result"]["value"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();

                        if accounts.is_empty() {
                            return format!("No SPL token accounts found for `{}`", address);
                        }

                        let mut output = format!(
                            "🪙 **SPL Token Balances** for `{}`\n\n",
                            address
                        );

                        let mut found_tokens = 0;
                        for account in &accounts {
                            let info = &account["account"]["data"]["parsed"]["info"];
                            let mint = info["mint"].as_str().unwrap_or("unknown");
                            let amount_str = info["tokenAmount"]["uiAmountString"]
                                .as_str()
                                .unwrap_or("0");
                            let decimals = info["tokenAmount"]["decimals"]
                                .as_u64()
                                .unwrap_or(0);

                            // Skip zero-balance accounts
                            let ui_amount = info["tokenAmount"]["uiAmount"]
                                .as_f64()
                                .unwrap_or(0.0);
                            if ui_amount == 0.0 {
                                continue;
                            }

                            found_tokens += 1;
                            let label = well_known_token(mint);
                            output.push_str(&format!(
                                "• **{}** — {} (decimals: {})\n  Mint: `{}`\n\n",
                                label, amount_str, decimals, mint
                            ));
                        }

                        if found_tokens == 0 {
                            return format!(
                                "No tokens with non-zero balance found for `{}`",
                                address
                            );
                        }

                        output
                    }
                    Err(e) => format!("Error parsing response: {}", e),
                }
            }
            Ok(resp) => format!("RPC error (HTTP {})", resp.status()),
            Err(e) => format!("Request failed: {}", e),
        }
    }
}

/// Map well-known Solana token mint addresses to human-readable labels.
fn well_known_token(mint: &str) -> &str {
    match mint {
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => "USDC",
        "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => "USDT",
        "So11111111111111111111111111111111111111112" => "Wrapped SOL",
        "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So" => "mSOL",
        "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj" => "stSOL",
        "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263" => "BONK",
        "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN" => "JUP",
        "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs" => "RAY",
        "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3" => "PYTH",
        "hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux" => "HNT",
        "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof" => "RNDR",
        _ => "Unknown Token",
    }
}

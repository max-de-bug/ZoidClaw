//! Solana blockchain tools.
//!
//! Provides on-chain data access via Solana's JSON-RPC API.
//! Makes ferrobot crypto-native with real wallet and token data.
//!
//! ## Architecture
//!
//! All tools share a common [`SolanaRpc`] helper that handles:
//! - HTTP client reuse (single `reqwest::Client` per tool instance)
//! - Address validation (base58, 32-44 chars)
//! - Consistent error formatting

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::Tool;

/// Lamports per SOL.
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// Solscan base URL for explorer links.
const SOLSCAN_BASE: &str = "https://solscan.io";

// ── Shared RPC helper ──────────────────────────────────────────────

/// Lightweight wrapper around `reqwest::Client` for Solana JSON-RPC calls.
///
/// Provides connection reuse, address validation, and consistent error
/// handling across all Solana tools.
struct SolanaRpc {
    client: Client,
    rpc_url: String,
}

impl SolanaRpc {
    fn new(client: Client, rpc_url: &str) -> Self {
        Self {
            client,
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Validate a Solana address (base58-encoded, 32–44 characters).
    fn validate_address(address: &str) -> Result<(), String> {
        if address.len() < 32 || address.len() > 44 {
            return Err(format!(
                "Invalid address length ({}). Solana addresses are 32–44 characters.",
                address.len()
            ));
        }
        if !address.chars().all(|c| {
            c.is_ascii_alphanumeric() && c != '0' && c != 'O' && c != 'I' && c != 'l'
                || "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(c)
        }) {
            return Err("Invalid base58 characters in address.".into());
        }
        Ok(())
    }

    /// Execute a JSON-RPC call and return the parsed response.
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });

        let resp = self
            .client
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Network error connecting to Solana RPC: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Solana RPC returned HTTP {} — the RPC endpoint may be overloaded or unreachable.",
                resp.status()
            ));
        }

        let data: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Solana RPC response: {}", e))?;

        if let Some(err) = data.get("error") {
            let msg = err["message"].as_str().unwrap_or("Unknown RPC error");
            return Err(format!("Solana RPC error: {}", msg));
        }

        Ok(data)
    }
}

// ── SolanaBalanceTool ───────────────────────────────────────────────

pub struct SolanaBalanceTool {
    rpc: SolanaRpc,
}

impl SolanaBalanceTool {
    pub fn new(client: Client, rpc_url: &str) -> Self {
        Self {
            rpc: SolanaRpc::new(client, rpc_url),
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
         Returns the balance in SOL and lamports with an explorer link."
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

        if let Err(e) = SolanaRpc::validate_address(address) {
            return format!("❌ {}", e);
        }

        debug!(address, "Fetching Solana balance");

        match self.rpc.call("getBalance", json!([address])).await {
            Ok(data) => {
                let lamports = data["result"]["value"].as_u64().unwrap_or(0);
                let sol = lamports as f64 / LAMPORTS_PER_SOL;
                format!(
                    "💰 **Solana Balance**\n\
                     Address: `{}`\n\
                     Balance: **{:.6} SOL** ({} lamports)\n\
                     🔗 [View on Solscan]({}/account/{})",
                    address, sol, lamports, SOLSCAN_BASE, address
                )
            }
            Err(e) => format!("❌ {}", e),
        }
    }
}

// ── SolanaTransactionsTool ──────────────────────────────────────────

pub struct SolanaTransactionsTool {
    rpc: SolanaRpc,
}

impl SolanaTransactionsTool {
    pub fn new(client: Client, rpc_url: &str) -> Self {
        Self {
            rpc: SolanaRpc::new(client, rpc_url),
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
         Returns the latest transactions with signatures, timestamps, and explorer links."
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
                    "type": "number",
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

        if let Err(e) = SolanaRpc::validate_address(address) {
            return format!("❌ {}", e);
        }

        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
            .unwrap_or(10)
            .min(20);

        debug!(address, limit, "Fetching Solana transactions");

        let params = json!([
            address,
            { "limit": limit, "commitment": "confirmed" }
        ]);

        match self.rpc.call("getSignaturesForAddress", params).await {
            Ok(data) => {
                let sigs: Vec<SignatureInfo> =
                    match serde_json::from_value(data["result"].clone()) {
                        Ok(s) => s,
                        Err(e) => return format!("❌ Error parsing transactions: {}", e),
                    };

                if sigs.is_empty() {
                    return format!("No transactions found for `{}`", address);
                }

                let mut output = format!(
                    "📜 **Recent Transactions** for `{}`\n\
                     🔗 [View all on Solscan]({}/account/{})\n\n",
                    address, SOLSCAN_BASE, address
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

                    let short_sig = &sig.signature[..16.min(sig.signature.len())];

                    output.push_str(&format!(
                        "{}. {} [`{}…`]({}/tx/{}) | slot {} | {}{}\n",
                        i + 1,
                        status,
                        short_sig,
                        SOLSCAN_BASE,
                        sig.signature,
                        sig.slot,
                        time_str,
                        memo_str,
                    ));
                }

                output
            }
            Err(e) => format!("❌ {}", e),
        }
    }
}

// ── SolanaTokenBalancesTool ─────────────────────────────────────────

pub struct SolanaTokenBalancesTool {
    rpc: SolanaRpc,
}

impl SolanaTokenBalancesTool {
    pub fn new(client: Client, rpc_url: &str) -> Self {
        Self {
            rpc: SolanaRpc::new(client, rpc_url),
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
         Returns token names, amounts, and explorer links."
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

        if let Err(e) = SolanaRpc::validate_address(address) {
            return format!("❌ {}", e);
        }

        debug!(address, "Fetching Solana token balances");

        let params = json!([
            address,
            { "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" },
            { "encoding": "jsonParsed" }
        ]);

        match self.rpc.call("getTokenAccountsByOwner", params).await {
            Ok(data) => {
                let accounts = data["result"]["value"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();

                if accounts.is_empty() {
                    return format!("No SPL token accounts found for `{}`", address);
                }

                let mut output = format!(
                    "🪙 **SPL Token Balances** for `{}`\n\
                     🔗 [View on Solscan]({}/account/{})\n\n",
                    address, SOLSCAN_BASE, address
                );

                let mut found_tokens = 0;
                for account in &accounts {
                    let info = &account["account"]["data"]["parsed"]["info"];
                    let mint = info["mint"].as_str().unwrap_or("unknown");
                    let amount_str = info["tokenAmount"]["uiAmountString"]
                        .as_str()
                        .unwrap_or("0");
                    let decimals = info["tokenAmount"]["decimals"].as_u64().unwrap_or(0);

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
                        "• **{}** — {} (decimals: {})\n  Mint: [`{}…`]({}/token/{})\n\n",
                        label,
                        amount_str,
                        decimals,
                        &mint[..8.min(mint.len())],
                        SOLSCAN_BASE,
                        mint,
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
            Err(e) => format!("❌ {}", e),
        }
    }
}

// ── Well-known token registry ──────────────────────────────────────

/// Map well-known Solana token mint addresses to human-readable labels.
fn well_known_token(mint: &str) -> &str {
    match mint {
        // Stablecoins
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => "USDC",
        "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => "USDT",

        // SOL variants
        "So11111111111111111111111111111111111111112" => "Wrapped SOL",
        "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So" => "mSOL",
        "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj" => "stSOL",
        "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn" => "jitoSOL",
        "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1" => "bSOL",

        // DeFi & Ecosystem
        "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN" => "JUP",
        "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs" => "RAY",
        "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE" => "ORCA",
        "jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL" => "JTO",
        "85VBFQZC9TZkfaptBWjvUw7YbZjy52A6mjtPGjstQAmQ" => "W",

        // Memecoins
        "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263" => "BONK",
        "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm" => "WIF",

        // Infrastructure
        "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3" => "PYTH",
        "hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux" => "HNT",
        "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof" => "RNDR",

        _ => "Unknown Token",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dump_rugcheck() {
        let url = "https://api.rugcheck.xyz/v1/tokens/DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263/report";
        let client = reqwest::Client::builder().user_agent("Mozilla/5.0").build().unwrap();
        let resp = client.get(url).send().await.unwrap();
        let json: serde_json::Value = resp.json().await.unwrap();
        std::fs::write("rugcheck_dump.json", serde_json::to_string_pretty(&json).unwrap()).unwrap();
    }
}

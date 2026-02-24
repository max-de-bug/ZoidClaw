//! Polymarket wallet tools.
//!
//! View EOA and proxy wallet addresses derived from the configured private key.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::config::PolymarketConfig;
use super::Tool;

// ── PolymarketWalletTool ───────────────────────────────────────────

/// View configured Polymarket wallet details (EOA and Proxy).
pub struct PolymarketWalletTool {
    config: PolymarketConfig,
}

impl PolymarketWalletTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketWalletTool {
    fn name(&self) -> &str {
        "polymarket_wallet"
    }

    fn description(&self) -> &str {
        "Check your configured Polymarket wallet details. Shows your \
         Externally Owned Account (EOA) address, your derived Proxy \
         Wallet address (used for trading), and your signature type."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: HashMap<String, Value>) -> String {
        let cli_args = vec!["wallet", "show"];
        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => {
                if output.contains("No wallet configured") || output.contains("Error:") {
                    return "❌ **No Wallet Configured**\n\n\
                            Run `polymarket_wallet_create` to create a new one, or \
                            `polymarket_wallet_import` to import an existing key."
                        .to_string();
                }
                format!("👛 **Polymarket Wallet**\n\n```text\n{}\n```", output)
            }
            Err(e) => format!("❌ Failed to retrieve wallet info: {e}"),
        }
    }
}

// ── PolymarketWalletCreateTool ─────────────────────────────────────

#[derive(Default)]
pub struct PolymarketWalletCreateTool;

impl PolymarketWalletCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketWalletCreateTool {
    fn name(&self) -> &str {
        "polymarket_wallet_create"
    }

    fn description(&self) -> &str {
        "Create a new Polymarket wallet. This generates a random private key, saves it to ~/.config/polymarket/config.json, and returns your new wallet details."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: HashMap<String, Value>) -> String {
        // Run with a dummy config since we don't need existing keys to create one
        let dummy_config = PolymarketConfig::default();
        let cli_args = vec!["wallet", "create"];
        
        match crate::tools::polymarket_common::run_polymarket_cli(&dummy_config, &cli_args).await {
            Ok(output) => format!("✅ **New Wallet Created Successfully!**\n\n```text\n{}\n```\n*⚠️ Your private key is securely stored in the config file. Do not share it!*", output),
            Err(e) => format!("❌ Failed to create wallet: {e}"),
        }
    }
}

// ── PolymarketWalletImportTool ─────────────────────────────────────

#[derive(Default)]
pub struct PolymarketWalletImportTool;

impl PolymarketWalletImportTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PolymarketWalletImportTool {
    fn name(&self) -> &str {
        "polymarket_wallet_import"
    }

    fn description(&self) -> &str {
        "Import an existing Polymarket private key into the local configuration."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "private_key": {
                    "type": "string",
                    "description": "The private key to import (optionally 0x prefixed)"
                }
            },
            "required": ["private_key"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(key) = args.get("private_key").and_then(|v| v.as_str()) else {
            return "❌ Missing parameter `private_key`.".to_string();
        };

        let dummy_config = PolymarketConfig::default();
        let cli_args = vec!["wallet", "import", key];
        
        match crate::tools::polymarket_common::run_polymarket_cli(&dummy_config, &cli_args).await {
            Ok(output) => format!("✅ **Wallet Imported Successfully!**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to import wallet: {e}"),
        }
    }
}

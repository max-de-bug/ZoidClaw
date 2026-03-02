//! Polymarket wallet tools.
//!
//! View EOA and proxy wallet addresses derived from the configured private key.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

use super::Tool;
use crate::config::PolymarketConfig;

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
        let (key, _sig, source) =
            crate::tools::polymarket_common::resolve_wallet_config(&self.config);

        if key.is_none() {
            return "❌ **No Wallet Configured**\n\n\
                    I couldn't find a Polymarket wallet key in your environment or config.\n\n\
                    **To fix this:**\n\
                    1. Run `polymarket_wallet_create` to generate a new one.\n\
                    2. Use `polymarket_wallet_import <key>` to use an existing one."
                .to_string();
        }

        let cli_args = vec!["wallet", "show"];
        let wallet_info = match crate::tools::polymarket_common::run_polymarket_cli(
            &self.config,
            &cli_args,
        )
        .await
        {
            Ok(output) => output,
            Err(e) => format!("❌ Failed to retrieve wallet info: {e}"),
        };

        // Check if API keys are configured (needed for CLOB trading/balance)
        let keys_args = vec!["clob", "api-keys"];
        let api_key_status = match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &keys_args).await {
            Ok(output) if output.contains("[]") || output.contains("No API keys found") => {
                "\n\n⚠️ **CLOB Authentication Missing**\n\
                 Your wallet is found, but you haven't \"connected\" it to the CLOB exchange yet.\n\
                 **Action required:** Run `polymarket_api_keys action=create` to generate your exchange credentials.\n\
                 ⚠️ **WAIT!** If you just tried this and it failed, do NOT retry automatically. The user may need a VPN. Inform the user."
                    .to_string()
            }
            Ok(_) => "\n\n✅ **CLOB Authentication Active**".to_string(),
            Err(_) => "".to_string(), // Silent fail for this check
        };

        format!(
            "👛 Polymarket Wallet (Source: {})\n\n{}{}",
            source.label(),
            wallet_info,
            api_key_status
        )
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
            Ok(output) => format!("✅ New Wallet Created Successfully!\n\n{}\n⚠️ Your private key is securely stored in the config file. Do not share it!", output),
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
            Ok(output) => format!(
                "✅ Wallet Imported Successfully!\n\n{}",
                output
            ),
            Err(e) => format!("❌ Failed to import wallet: {e}"),
        }
    }
}

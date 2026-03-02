//! Polymarket CTF (Conditional Token Framework) tools.
//!
//! On-chain operations: split USDC into YES/NO tokens, merge tokens
//! back to USDC, and redeem winning tokens after market resolution.
//! Requires wallet + MATIC for gas on Polygon.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::polymarket_common::require_wallet;
use super::Tool;
use crate::config::PolymarketConfig;

// ── PolymarketCtfSplitTool ─────────────────────────────────────────

/// Split USDC into conditional YES/NO tokens.
pub struct PolymarketCtfSplitTool {
    config: PolymarketConfig,
}

impl PolymarketCtfSplitTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketCtfSplitTool {
    fn name(&self) -> &str {
        "polymarket_ctf_split"
    }

    fn description(&self) -> &str {
        "Split USDC collateral into conditional YES/NO tokens for a \
         Polymarket market. This is an on-chain Polygon transaction. \
         Requires wallet + MATIC for gas. ⚠️ Spends real USDC."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "condition_id": {
                    "type": "string",
                    "description": "Market condition ID (0x-prefixed hex)"
                },
                "amount": {
                    "type": "string",
                    "description": "Amount in USDC to split (e.g. '10' for $10)"
                }
            },
            "required": ["condition_id", "amount"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let _key = match require_wallet(&self.config) {
            Ok(k) => k,
            Err(e) => return e,
        };

        let Some(condition_id) = args.get("condition_id").and_then(|v| v.as_str()) else {
            return "Error: 'condition_id' is required".into();
        };
        let Some(amount) = args.get("amount").and_then(|v| v.as_str()) else {
            return "Error: 'amount' is required".into();
        };

        debug!(condition_id, amount, "CTF split request");

        format!(
            "🔀 **CTF Split** (preview)\n\n\
             Condition: `{condition_id}`\n\
             Amount: **${amount} USDC** → YES + NO tokens\n\n\
             ⚠️ On-chain split requires alloy provider integration.\n\
             Use `polymarket ctf split --condition {condition_id} --amount {amount}` CLI.",
            condition_id = condition_id,
            amount = amount,
        )
    }
}

// ── PolymarketCtfMergeTool ─────────────────────────────────────────

/// Merge conditional tokens back into USDC.
pub struct PolymarketCtfMergeTool {
    config: PolymarketConfig,
}

impl PolymarketCtfMergeTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketCtfMergeTool {
    fn name(&self) -> &str {
        "polymarket_ctf_merge"
    }

    fn description(&self) -> &str {
        "Merge conditional YES/NO tokens back into USDC collateral. \
         This is the reverse of split. On-chain Polygon transaction. \
         Requires wallet + MATIC for gas."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "condition_id": {
                    "type": "string",
                    "description": "Market condition ID (0x-prefixed hex)"
                },
                "amount": {
                    "type": "string",
                    "description": "Amount in USDC to merge back (e.g. '10' for $10)"
                }
            },
            "required": ["condition_id", "amount"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let _key = match require_wallet(&self.config) {
            Ok(k) => k,
            Err(e) => return e,
        };

        let Some(condition_id) = args.get("condition_id").and_then(|v| v.as_str()) else {
            return "Error: 'condition_id' is required".into();
        };
        let Some(amount) = args.get("amount").and_then(|v| v.as_str()) else {
            return "Error: 'amount' is required".into();
        };

        debug!(condition_id, amount, "CTF merge request");

        format!(
            "🔀 **CTF Merge** (preview)\n\n\
             Condition: `{condition_id}`\n\
             Amount: YES + NO tokens → **${amount} USDC**\n\n\
             ⚠️ On-chain merge requires alloy provider integration.\n\
             Use `polymarket ctf merge --condition {condition_id} --amount {amount}` CLI.",
            condition_id = condition_id,
            amount = amount,
        )
    }
}

// ── PolymarketCtfRedeemTool ────────────────────────────────────────

/// Redeem winning conditional tokens after market resolution.
pub struct PolymarketCtfRedeemTool {
    config: PolymarketConfig,
}

impl PolymarketCtfRedeemTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketCtfRedeemTool {
    fn name(&self) -> &str {
        "polymarket_ctf_redeem"
    }

    fn description(&self) -> &str {
        "Redeem winning conditional tokens for USDC after a market has \
         resolved. On-chain Polygon transaction. Requires wallet + MATIC."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "condition_id": {
                    "type": "string",
                    "description": "Resolved market condition ID (0x-prefixed hex)"
                }
            },
            "required": ["condition_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let _key = match require_wallet(&self.config) {
            Ok(k) => k,
            Err(e) => return e,
        };

        let Some(condition_id) = args.get("condition_id").and_then(|v| v.as_str()) else {
            return "Error: 'condition_id' is required".into();
        };

        debug!(condition_id, "CTF redeem request");

        format!(
            "💰 **CTF Redeem** (preview)\n\n\
             Condition: `{condition_id}`\n\
             Action: Redeem winning tokens → USDC\n\n\
             ⚠️ On-chain redeem requires alloy provider integration.\n\
             Use `polymarket ctf redeem --condition {condition_id}` CLI.",
            condition_id = condition_id,
        )
    }
}

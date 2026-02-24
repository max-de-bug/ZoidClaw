//! Polymarket contract approval tools.
//!
//! Check and set ERC-20 (USDC) and ERC-1155 (CTF token) approvals
//! required before trading on Polymarket.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use crate::config::PolymarketConfig;
use super::polymarket_common::require_wallet;
use super::Tool;

// ── PolymarketApproveTool ──────────────────────────────────────────

/// Check or set Polymarket contract approvals.
pub struct PolymarketApproveTool {
    config: PolymarketConfig,
}

impl PolymarketApproveTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketApproveTool {
    fn name(&self) -> &str {
        "polymarket_approve"
    }

    fn description(&self) -> &str {
        "Check or set ERC-20 (USDC) and ERC-1155 (CTF) contract approvals \
         required before trading on Polymarket. Use 'check' to view status, \
         'set' to approve all contracts. Setting approvals sends on-chain \
         transactions and requires MATIC for gas."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["check", "set"],
                    "description": "Action: 'check' to view approval status, 'set' to approve all contracts"
                },
                "address": {
                    "type": "string",
                    "description": "Optional wallet address for check (defaults to configured wallet)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(action) = args.get("action").and_then(|v| v.as_str()) else {
            return "Error: 'action' is required (check or set)".into();
        };

        if action == "set" {
            let _key = match require_wallet(&self.config) {
                Ok(k) => k,
                Err(e) => return e,
            };
        }

        let address = args.get("address").and_then(|v| v.as_str());
        debug!(action, ?address, "Polymarket approval operation");

        match action {
            "check" => "✅ **Approval Check**\n\n\
                 Checking contract approvals for Polymarket...\n\n\
                 ⚠️ Approval checking requires alloy provider integration.\n\
                 Use `polymarket approve check` CLI for now.".to_string(),
            "set" => "🔓 **Set Approvals** (preview)\n\n\
                 This will send 6 on-chain transactions to approve:\n\
                 • USDC (ERC-20) for Exchange contract\n\
                 • USDC (ERC-20) for Neg Risk Exchange\n\
                 • CTF (ERC-1155) for Exchange contract\n\
                 • CTF (ERC-1155) for Neg Risk Exchange\n\
                 • Neg Risk CTF for Neg Risk Exchange\n\
                 • Neg Risk CTF for Neg Risk Adapter\n\n\
                 💰 Requires MATIC for gas on Polygon.\n\n\
                 ⚠️ On-chain approvals require alloy provider integration.\n\
                 Use `polymarket approve set` CLI for now.".to_string(),
            _ => format!("Error: unknown action '{action}'. Use 'check' or 'set'."),
        }
    }
}

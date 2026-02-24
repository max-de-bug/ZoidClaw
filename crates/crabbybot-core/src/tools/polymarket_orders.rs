//! Polymarket order management tools (authenticated).
//!
//! View open orders, cancel orders, and check CLOB balances.
//! Requires a configured wallet for order management.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use crate::config::PolymarketConfig;
use super::Tool;

// ── PolymarketMyOrdersTool ─────────────────────────────────────────

/// View open orders on the Polymarket CLOB.
pub struct PolymarketMyOrdersTool {
    config: PolymarketConfig,
}

impl PolymarketMyOrdersTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketMyOrdersTool {
    fn name(&self) -> &str {
        "polymarket_my_orders"
    }

    fn description(&self) -> &str {
        "View your open orders on the Polymarket CLOB. Optionally filter \
         by market condition ID. Requires a configured wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "market": {
                    "type": "string",
                    "description": "Optional: filter by market condition ID (0x...)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let market = args.get("market").and_then(|v| v.as_str());
        debug!(?market, "Fetching Polymarket orders");

        let mut cli_args = vec!["clob", "orders"];
        let market_string;
        if let Some(m) = market {
            market_string = m.to_string();
            cli_args.push("--market");
            cli_args.push(&market_string);
        }
        
        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("📋 **My Orders**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to fetch orders: {e}"),
        }
    }
}

// ── PolymarketCancelOrderTool ──────────────────────────────────────

/// Cancel orders on the Polymarket CLOB.
pub struct PolymarketCancelOrderTool {
    config: PolymarketConfig,
}

impl PolymarketCancelOrderTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketCancelOrderTool {
    fn name(&self) -> &str {
        "polymarket_cancel_order"
    }

    fn description(&self) -> &str {
        "Cancel one or all orders on the Polymarket CLOB. Specify an \
         order ID to cancel a specific order, or use 'all' to cancel \
         everything. Requires a configured wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "order_id": {
                    "type": "string",
                    "description": "Order ID to cancel, or 'all' to cancel all orders"
                }
            },
            "required": ["order_id"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(order_id) = args.get("order_id").and_then(|v| v.as_str()) else {
            return "Error: 'order_id' is required".into();
        };

        debug!(order_id, "Cancelling Polymarket order");

        let cli_args = if order_id.eq_ignore_ascii_case("all") {
            vec!["clob", "cancel-all"]
        } else {
            vec!["clob", "cancel", order_id]
        };

        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("✅ **Order Cancellation Result:**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to cancel order(s): {e}"),
        }
    }
}

// ── PolymarketBalanceTool ──────────────────────────────────────────

/// Check CLOB balances (collateral and conditional tokens).
pub struct PolymarketBalanceTool {
    config: PolymarketConfig,
}

impl PolymarketBalanceTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketBalanceTool {
    fn name(&self) -> &str {
        "polymarket_balance"
    }

    fn description(&self) -> &str {
        "Check your Polymarket CLOB balance. Shows USDC collateral \
         and conditional token balances. Requires a configured wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "asset_type": {
                    "type": "string",
                    "enum": ["collateral", "conditional"],
                    "description": "Asset type to check (default: collateral/USDC)"
                },
                "token_id": {
                    "type": "string",
                    "description": "Token ID for conditional balance check"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let asset_type_str = args
            .get("asset_type")
            .and_then(|v| v.as_str())
            .unwrap_or("collateral");
            
        let token_id_str = args.get("token_id").and_then(|v| v.as_str());

        let mut cli_args = vec!["clob", "balance", "--asset-type", asset_type_str];
        let token_string;
        if let Some(t) = token_id_str {
            token_string = t.to_string();
            cli_args.push("--token");
            cli_args.push(&token_string);
        }

        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("💰 **Polymarket Balance ({})**\n\n```text\n{}\n```", asset_type_str, output),
            Err(e) => format!("❌ Failed to fetch balance: {e}"),
        }
    }
}

// ── PolymarketRewardsTool ──────────────────────────────────────────

/// Check Polymarket reward earnings.
pub struct PolymarketRewardsTool {
    config: PolymarketConfig,
}

impl PolymarketRewardsTool {
    pub fn new(config: PolymarketConfig) -> Self { Self { config } }
}

#[async_trait]
impl Tool for PolymarketRewardsTool {
    fn name(&self) -> &str { "polymarket_rewards" }

    fn description(&self) -> &str {
        "View your Polymarket reward earnings for a specific date, including \
         market-making rewards and current reward programs. Requires a wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["earnings", "current", "percentages"],
                    "description": "Action: 'earnings' for daily earnings, 'current' for active programs, 'percentages' for rates"
                },
                "date": {
                    "type": "string",
                    "description": "Date for earnings (YYYY-MM-DD format, for earnings action)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("earnings");
        let date = args.get("date").and_then(|v| v.as_str());
        debug!(action, ?date, "Polymarket rewards");

        let mut cli_args = vec!["clob"];
        let date_string;
        
        match action {
            "earnings" => cli_args.push("earnings"),
            "current" => cli_args.push("current-rewards"),
            "percentages" => cli_args.push("reward-percentages"),
            _ => return format!("❌ Unknown action '{action}'."),
        };

        if let Some(d) = date {
            date_string = d.to_string();
            if action == "earnings" {
                cli_args.push("--date");
                cli_args.push(&date_string);
            }
        }

        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("💎 **Polymarket Rewards ({})**\n\n```text\n{}\n```", action, output),
            Err(e) => format!("❌ Failed to fetch rewards: {e}"),
        }
    }
}

// ── PolymarketNotificationsTool ────────────────────────────────────

/// View Polymarket CLOB notifications.
pub struct PolymarketNotificationsTool {
    config: PolymarketConfig,
}

impl PolymarketNotificationsTool {
    pub fn new(config: PolymarketConfig) -> Self { Self { config } }
}

#[async_trait]
impl Tool for PolymarketNotificationsTool {
    fn name(&self) -> &str { "polymarket_notifications" }

    fn description(&self) -> &str {
        "View your Polymarket CLOB notifications (order fills, liquidations, etc.). \
         Requires a configured wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: HashMap<String, Value>) -> String {
        debug!("Fetching notifications");

        let cli_args = vec!["clob", "notifications"];
        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("🔔 **Polymarket Notifications**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to fetch notifications: {e}"),
        }
    }
}

// ── PolymarketApiKeysTool ──────────────────────────────────────────

/// Manage Polymarket CLOB API keys.
pub struct PolymarketApiKeysTool {
    config: PolymarketConfig,
}

impl PolymarketApiKeysTool {
    pub fn new(config: PolymarketConfig) -> Self { Self { config } }
}

#[async_trait]
impl Tool for PolymarketApiKeysTool {
    fn name(&self) -> &str { "polymarket_api_keys" }

    fn description(&self) -> &str {
        "Manage Polymarket CLOB API keys. List existing keys, create new ones, \
         or delete the current key. Requires a configured wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "create"],
                    "description": "Action: list or create API key (delete requires interactive auth)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        debug!(action, "Polymarket API keys");

        let cli_args = match action {
            "list" => vec!["clob", "api-keys"],
            "create" => vec!["clob", "create-api-key"],
            "delete" => return format!("❌ Action 'delete' is interactive and requires terminal usage. Type `cargo run -p polymarket-cli -- clob delete-api-key` in terminal."),
            _ => return format!("❌ Unknown action '{action}'."),
        };

        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("🔑 **API Keys ({})**\n\n```text\n{}\n```", action, output),
            Err(e) => format!("❌ Failed API key action '{action}': {e}"),
        }
    }
}

// ── PolymarketAccountStatusTool ────────────────────────────────────

/// Check Polymarket CLOB account status.
pub struct PolymarketAccountStatusTool {
    config: PolymarketConfig,
}

impl PolymarketAccountStatusTool {
    pub fn new(config: PolymarketConfig) -> Self { Self { config } }
}

#[async_trait]
impl Tool for PolymarketAccountStatusTool {
    fn name(&self) -> &str { "polymarket_account_status" }

    fn description(&self) -> &str {
        "Check your Polymarket CLOB account status, including API access level \
         and account restrictions. Requires a configured wallet."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: HashMap<String, Value>) -> String {
        debug!("Checking account status");

        let cli_args = vec!["clob", "account-status"];
        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("👤 **Account Status**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to fetch account status: {e}"),
        }
    }
}

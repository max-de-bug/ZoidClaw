//! Polymarket CLOB trading tools (authenticated).
//!
//! Place limit and market orders on the Polymarket CLOB.
//! Requires a configured wallet (`POLYMARKET_PRIVATE_KEY` env var).

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use crate::config::PolymarketConfig;
use super::Tool;

// ── PolymarketCreateOrderTool ──────────────────────────────────────

/// Place a limit order on the Polymarket CLOB.
pub struct PolymarketCreateOrderTool {
    config: PolymarketConfig,
}

impl PolymarketCreateOrderTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketCreateOrderTool {
    fn name(&self) -> &str {
        "polymarket_create_order"
    }

    fn description(&self) -> &str {
        "Place a limit order on Polymarket's CLOB. Specify a token ID, \
         side (buy/sell), price (0-1.00), and size (number of shares). \
         Requires a configured Polymarket wallet. \
         ⚠️ This will place a real order with real funds."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "Token ID to trade (numeric string)"
                },
                "side": {
                    "type": "string",
                    "enum": ["buy", "sell"],
                    "description": "Order side: buy or sell"
                },
                "price": {
                    "type": "string",
                    "description": "Price per share (0.01 to 0.99, e.g. '0.50' for 50¢)"
                },
                "size": {
                    "type": "string",
                    "description": "Number of shares (e.g. '10' for 10 shares)"
                },
                "order_type": {
                    "type": "string",
                    "enum": ["GTC", "FOK", "GTD", "FAK"],
                    "description": "Order type (default: GTC). GTC=Good-Til-Cancelled, FOK=Fill-Or-Kill, GTD=Good-Til-Date, FAK=Fill-And-Kill"
                }
            },
            "required": ["token_id", "side", "price", "size"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id_str) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' is required".into();
        };
        let Some(side_str) = args.get("side").and_then(|v| v.as_str()) else {
            return "Error: 'side' is required".into();
        };
        let Some(price_str) = args.get("price").and_then(|v| v.as_str()) else {
            return "Error: 'price' is required".into();
        };
        let Some(size_str) = args.get("size").and_then(|v| v.as_str()) else {
            return "Error: 'size' is required".into();
        };
        let order_type_str = args.get("order_type").and_then(|v| v.as_str());

        debug!(%token_id_str, ?side_str, %price_str, %size_str, "Creating Polymarket limit order");

        let mut cli_args = vec![
            "clob", "create-order",
            "--token", token_id_str,
            "--side", side_str,
            "--price", price_str,
            "--size", size_str,
        ];
        
        if let Some(ot) = order_type_str {
            cli_args.push("--order-type");
            cli_args.push(ot);
        }

        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("✅ **Limit Order Result:**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to post limit order: {e}"),
        }
    }
}

// ── PolymarketMarketOrderTool ──────────────────────────────────────

/// Place a market order on the Polymarket CLOB.
pub struct PolymarketMarketOrderTool {
    config: PolymarketConfig,
}

impl PolymarketMarketOrderTool {
    pub fn new(config: PolymarketConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for PolymarketMarketOrderTool {
    fn name(&self) -> &str {
        "polymarket_market_order"
    }

    fn description(&self) -> &str {
        "Place a market order on Polymarket's CLOB. Buys or sells at the \
         best available price. Specify a token ID, side, and dollar amount. \
         Requires a configured Polymarket wallet. \
         ⚠️ This will execute immediately with real funds."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "Token ID to trade (numeric string)"
                },
                "side": {
                    "type": "string",
                    "enum": ["buy", "sell"],
                    "description": "Order side: buy or sell"
                },
                "amount": {
                    "type": "string",
                    "description": "Dollar amount for buys (e.g. '5' for $5 USDC), or share count for sells"
                }
            },
            "required": ["token_id", "side", "amount"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(token_id_str) = args.get("token_id").and_then(|v| v.as_str()) else {
            return "Error: 'token_id' is required".into();
        };
        let Some(side_str) = args.get("side").and_then(|v| v.as_str()) else {
            return "Error: 'side' is required".into();
        };
        let Some(amount_str) = args.get("amount").and_then(|v| v.as_str()) else {
            return "Error: 'amount' is required".into();
        };

        debug!(%token_id_str, ?side_str, %amount_str, "Creating Polymarket market order");

        let cli_args = vec![
            "clob", "market-order",
            "--token", token_id_str,
            "--side", side_str,
            "--amount", amount_str,
        ];

        match crate::tools::polymarket_common::run_polymarket_cli(&self.config, &cli_args).await {
            Ok(output) => format!("✅ **Market Order Result:**\n\n```text\n{}\n```", output),
            Err(e) => format!("❌ Failed to post market order: {e}"),
        }
    }
}

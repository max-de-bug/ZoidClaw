//! Tool system: trait, registry, and built-in tool implementations.
//!
//! Every tool implements the `Tool` trait and registers itself in the
//! `ToolRegistry`. The agent loop queries the registry for available
//! tools and dispatches tool calls by name.

pub mod alpha_summary;
pub mod discovery;
pub mod filesystem;
pub mod polymarket;
pub mod polymarket_approve;
pub mod polymarket_bridge;
pub mod polymarket_comments;
pub mod polymarket_common;
pub mod polymarket_ctf;
pub mod polymarket_data;
pub mod polymarket_events;
pub mod polymarket_orderbook;
pub mod polymarket_orders;
pub mod polymarket_prices;
pub mod polymarket_profiles;
pub mod polymarket_series;
pub mod polymarket_sports;
pub mod polymarket_status;
pub mod polymarket_tags;
pub mod polymarket_trade;
pub mod polymarket_wallet;
pub mod pumpfun;
pub mod pumpfun_buy;
pub mod rugcheck;
pub mod schedule;
pub mod sentiment;
pub mod shell;
pub mod solana;
pub mod web;

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, error};

use crate::provider::types::{ToolDefinition, ToolFunctionDef};

/// Trait that all agent tools must implement.
///
/// Tools are capabilities the agent can invoke (read files, run commands, etc.).
/// Each tool declares its name, description, JSON Schema parameters, and
/// an async `execute` method.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name used in function calls (e.g., "read_file").
    fn name(&self) -> &str;

    /// Human-readable description of what the tool does.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters.
    fn parameters(&self) -> Value;

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: HashMap<String, Value>) -> String;
}

/// High-level categories representing user intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IntentCategory {
    /// Broad analysis, web search, scraping
    Research,
    /// Creating, reading, running scripts, general system stuff
    System,
    /// Viewing Polymarket data, markets, events (read-only)
    PolymarketRead,
    /// Trading, viewing wallet, managing Polymarket positions
    PolymarketTrade,
    /// RugCheck, PumpFun, token/coin specific things
    CryptoTokens,
    /// Fallback for general conversation
    #[default]
    General,
}

impl IntentCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::System => "system",
            Self::PolymarketRead => "polymarket_read",
            Self::PolymarketTrade => "polymarket_trade",
            Self::CryptoTokens => "crypto_tokens",
            Self::General => "general",
        }
    }
}

/// Dynamic registry for agent tools.
///
/// Allows runtime registration and lookup of tools by name.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, (Box<dyn Tool>, IntentCategory)>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool with a specific intent category.
    pub fn register(&mut self, tool: Box<dyn Tool>, category: IntentCategory) {
        debug!(tool = tool.name(), category = category.as_str(), "Registered tool");
        self.tools.insert(tool.name().to_string(), (tool, category));
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|(t, _)| t.as_ref())
    }

    /// Check if a tool is registered.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Execute a tool by name with the given arguments.
    pub async fn execute(&self, name: &str, args: HashMap<String, Value>) -> String {
        match self.tools.get(name) {
            Some((tool, _)) => {
                debug!(tool = name, "Executing tool");
                tool.execute(args).await
            }
            None => {
                error!(tool = name, "Tool not found");
                format!("Error: Tool '{}' not found", name)
            }
        }
    }

    /// Get all tool definitions for a given category.
    pub fn definitions_for(&self, category: IntentCategory) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .filter(|(_, cat)| *cat == category || *cat == IntentCategory::General) // Always include general
            .map(|(tool, _)| ToolDefinition {
                def_type: "function".into(),
                function: ToolFunctionDef {
                    name: tool.name().into(),
                    description: tool.description().into(),
                    parameters: tool.parameters(),
                },
            })
            .collect()
    }

    /// Get all tool definitions (ignoring categories).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|(tool, _)| ToolDefinition {
                def_type: "function".into(),
                function: ToolFunctionDef {
                    name: tool.name().into(),
                    description: tool.description().into(),
                    parameters: tool.parameters(),
                },
            })
            .collect()
    }

    /// Get the list of registered tool names.
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn description(&self) -> &str {
            "A dummy tool for testing"
        }
        fn parameters(&self) -> Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _args: HashMap<String, Value>) -> String {
            "dummy result".into()
        }
    }

    #[tokio::test]
    async fn test_register_and_execute() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool), IntentCategory::General);

        assert!(registry.has("dummy"));
        assert_eq!(registry.len(), 1);

        let result = registry.execute("dummy", HashMap::new()).await;
        assert_eq!(result, "dummy result");
    }

    #[tokio::test]
    async fn test_missing_tool() {
        let registry = ToolRegistry::new();
        let result = registry.execute("nonexistent", HashMap::new()).await;
        assert!(result.contains("not found"));
    }
}

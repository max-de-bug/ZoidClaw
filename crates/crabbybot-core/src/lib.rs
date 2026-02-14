//! 🦀 crabbybot-core: Core library for the crabbybot AI assistant.
//!
//! This crate contains all the building blocks for an ultra-lightweight AI assistant:
//!
//! - [`config`] — Typed configuration loading from JSON
//! - [`provider`] — LLM provider trait and OpenAI-compatible implementation
//! - [`bus`] — Async message bus for channel-agent decoupling
//! - [`tools`] — Tool trait, registry, and built-in filesystem/shell/web tools
//! - [`agent`] — Agent loop, memory, skills, and context building
//! - [`session`] — Conversation session persistence (JSONL)
//! - [`cron`] — Scheduled task management
//!
//! # Quick Start
//!
//! ```no_run
//! use crabbybot_core::config::Config;
//! use crabbybot_core::provider::openai::OpenAiProvider;
//! use crabbybot_core::agent::{AgentLoop, AgentConfig};
//! use crabbybot_core::tools::ToolRegistry;
//!
//! // Load configuration
//! let config = Config::load().unwrap();
//!
//! // Create a provider
//! let (name, entry) = config.providers.find_active().unwrap();
//! let provider = OpenAiProvider::new(name, &entry.api_key, None, &config.agents.defaults.model);
//!
//! // Set up tools and agent
//! let tools = ToolRegistry::new();
//! let agent_config = AgentConfig {
//!     model: config.agents.defaults.model.clone(),
//!     max_tokens: config.agents.defaults.max_tokens,
//!     temperature: config.agents.defaults.temperature,
//!     max_iterations: config.agents.defaults.max_tool_iterations,
//!     workspace: config.workspace_path(),
//! };
//!
//! let mut agent = AgentLoop::new(Box::new(provider), tools, agent_config);
//! ```

pub mod agent;
pub mod bus;
pub mod config;
pub mod cron;
pub mod gateway;
pub mod provider;
pub mod session;
pub mod tools;

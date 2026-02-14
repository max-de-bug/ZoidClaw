//! Configuration module for crabbybot.
//!
//! Loads typed configuration from `~/.crabbybot/config.json`.
//! All fields use `serde` for zero-boilerplate deserialization.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Root configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub providers: ProvidersConfig,
    pub agents: AgentsConfig,
    pub tools: ToolsConfig,
    pub channels: ChannelsConfig,
    pub gateway: GatewayConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: ProvidersConfig::default(),
            agents: AgentsConfig::default(),
            tools: ToolsConfig::default(),
            channels: ChannelsConfig::default(),
            gateway: GatewayConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from the default path (`~/.crabbybot/config.json`).
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::default_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let config: Config = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Load configuration from a specific path.
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Get the default config file path.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".crabbybot")
            .join("config.json")
    }

    /// Get the default config directory path.
    pub fn config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".crabbybot")
    }

    /// Get the resolved workspace path.
    pub fn workspace_path(&self) -> PathBuf {
        let raw = &self.agents.defaults.workspace;
        if raw.starts_with("~/") || raw.starts_with("~\\") {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(&raw[2..])
        } else {
            PathBuf::from(raw)
        }
    }

    /// Write the default config template to disk.
    pub fn write_default_template() -> anyhow::Result<PathBuf> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let template = serde_json::json!({
            "providers": {
                "openrouter": {
                    "apiKey": "sk-or-v1-YOUR_KEY_HERE"
                }
            },
            "agents": {
                "defaults": {
                    "model": "anthropic/claude-sonnet-4-5"
                }
            }
        });

        std::fs::write(&path, serde_json::to_string_pretty(&template)?)?;
        Ok(path)
    }
}

// ── Provider Configuration ──────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderEntry {
    pub api_key: String,
    pub api_base: Option<String>,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProvidersConfig {
    pub openrouter: Option<ProviderEntry>,
    pub anthropic: Option<ProviderEntry>,
    pub openai: Option<ProviderEntry>,
    pub deepseek: Option<ProviderEntry>,
    pub groq: Option<ProviderEntry>,
    pub gemini: Option<ProviderEntry>,
    pub vllm: Option<ProviderEntry>,
}

impl ProvidersConfig {
    /// Find the first configured provider (has a non-empty API key).
    pub fn find_active(&self) -> Option<(&str, &ProviderEntry)> {
        let candidates: Vec<(&str, &Option<ProviderEntry>)> = vec![
            ("openrouter", &self.openrouter),
            ("anthropic", &self.anthropic),
            ("openai", &self.openai),
            ("deepseek", &self.deepseek),
            ("groq", &self.groq),
            ("gemini", &self.gemini),
            ("vllm", &self.vllm),
        ];

        for (name, entry) in candidates {
            if let Some(e) = entry {
                if !e.api_key.is_empty() {
                    return Some((name, e));
                }
            }
        }
        None
    }
}

// ── Agent Configuration ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentDefaults {
    pub workspace: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub max_tool_iterations: u32,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            workspace: "~/.crabbybot/workspace".into(),
            model: "anthropic/claude-sonnet-4-5".into(),
            max_tokens: 8192,
            temperature: 0.7,
            max_tool_iterations: 20,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    pub defaults: AgentDefaults,
}

// ── Tools Configuration ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolsConfig {
    pub restrict_to_workspace: bool,
    pub web_search: WebSearchConfig,
    pub exec: ExecConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            restrict_to_workspace: false,
            web_search: WebSearchConfig::default(),
            exec: ExecConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebSearchConfig {
    pub api_key: String,
    pub max_results: u32,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            max_results: 5,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExecConfig {
    pub timeout_seconds: u64,
    pub allowed_commands: Vec<String>,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            allowed_commands: Vec::new(),
        }
    }
}

// ── Channels Configuration ──────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ChannelsConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TelegramConfig {
    pub enabled: bool,
    pub token: String,
    pub allow_from: Vec<String>,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: String::new(),
            allow_from: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiscordConfig {
    pub enabled: bool,
    pub token: String,
    pub allow_from: Vec<String>,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: String::new(),
            allow_from: Vec::new(),
        }
    }
}

// ── Gateway Configuration ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 18790,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.agents.defaults.model, "anthropic/claude-sonnet-4-5");
        assert_eq!(config.agents.defaults.max_tokens, 8192);
        assert!(!config.tools.restrict_to_workspace);
    }

    #[test]
    fn test_deserialize_minimal_json() {
        let json = r#"{"providers": {"openrouter": {"apiKey": "test-key"}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let entry = config.providers.openrouter.unwrap();
        assert_eq!(entry.api_key, "test-key");
    }

    #[test]
    fn test_find_active_provider() {
        let json = r#"{"providers": {"anthropic": {"apiKey": "sk-ant-xxx"}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let (name, entry) = config.providers.find_active().unwrap();
        assert_eq!(name, "anthropic");
        assert_eq!(entry.api_key, "sk-ant-xxx");
    }
}

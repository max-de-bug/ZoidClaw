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
    /// Load configuration.
    ///
    /// Priority:
    /// 1. local `config.json` in current directory
    /// 2. `~/.ferrobot/config.json`
    /// 3. `~/.crabbybot/config.json`
    pub fn load() -> anyhow::Result<Self> {
        let paths = vec![
            PathBuf::from("config.json"),
            Self::ferrobot_path(),
            Self::default_path(),
        ];

        for path in paths {
            if path.exists() {
                tracing::debug!("Loading config from: {}", path.display());
                let content = std::fs::read_to_string(&path)?;
                let config: Config = serde_json::from_str(&content)?;
                return Ok(config);
            }
        }

        // No config found, return default with placeholders
        Ok(Config::default())
    }

    /// Load configuration from a specific path.
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Get the path to `~/.ferrobot/config.json`.
    pub fn ferrobot_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".ferrobot")
            .join("config.json")
    }

    /// Get the default config file path (`~/.crabbybot/config.json`).
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

    /// Validate configuration and return actionable error messages.
    ///
    /// Checks that:
    /// - At least one provider has a real (non-placeholder) API key
    /// - The default model is not empty
    /// - Enabled channels have a token configured
    pub fn validate(&self) -> std::result::Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Check providers — must have at least one real key.
        if self.providers.find_active().is_none() {
            errors.push(
                "No LLM provider configured with a real API key. \
                 Edit config.json and replace the placeholder key."
                    .into(),
            );
        }

        // Check model.
        if self.agents.defaults.model.is_empty() {
            errors.push("agents.defaults.model is empty. Specify a model name.".into());
        }

        // Check channels — enabled channels must have a token.
        if let Some(ref tg) = self.channels.telegram {
            if tg.enabled && (tg.token.is_empty() || tg.token.contains("YOUR_")) {
                errors.push(
                    "Telegram is enabled but the bot token is missing or a placeholder. \
                     Set channels.telegram.token in config.json."
                        .into(),
                );
            }
        }
        if let Some(ref dc) = self.channels.discord {
            if dc.enabled && (dc.token.is_empty() || dc.token.contains("YOUR_")) {
                errors.push(
                    "Discord is enabled but the bot token is missing or a placeholder. \
                     Set channels.discord.token in config.json."
                        .into(),
                );
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

// ── Provider Configuration ──────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderEntry {
    pub api_key: String,
    pub api_base: Option<String>,
    pub model: Option<String>,
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
    /// Find the first configured provider (has a non-empty, non-placeholder API key).
    pub fn find_active(&self) -> Option<(&str, &ProviderEntry)> {
        self.find_all_active().into_iter().next()
    }

    /// Find all configured providers that have a real API key.
    pub fn find_all_active(&self) -> Vec<(&'static str, &ProviderEntry)> {
        let placeholder_prefixes = ["YOUR_", "sk-or-v1-YOUR", "sk-YOUR", "sk-ant-YOUR"];
        
        let candidates: Vec<(&'static str, &Option<ProviderEntry>)> = vec![
            ("openrouter", &self.openrouter),
            ("anthropic", &self.anthropic),
            ("openai", &self.openai),
            ("deepseek", &self.deepseek),
            ("groq", &self.groq),
            ("gemini", &self.gemini),
            ("vllm", &self.vllm),
        ];

        let mut active = Vec::new();
        for (name, entry) in candidates {
            if let Some(e) = entry {
                if !e.api_key.is_empty() && !placeholder_prefixes.iter().any(|p| e.api_key.contains(p)) {
                    active.push((name, e));
                }
            }
        }
        active
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

    #[test]
    fn test_validate_catches_placeholder_key() {
        let json = r#"{"providers": {"openrouter": {"apiKey": "sk-or-v1-YOUR_KEY_HERE"}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("real API key")));
    }

    #[test]
    fn test_validate_passes_with_real_key() {
        let json = r#"{"providers": {"openai": {"apiKey": "sk-abc123def456"}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_catches_empty_model() {
        let mut config = Config::default();
        config.agents.defaults.model = String::new();
        // Also need a real key so the model error is the one we catch.
        config.providers.openai = Some(ProviderEntry {
            api_key: "sk-real-key-123".into(),
            api_base: None,
            extra_headers: Default::default(),
        });
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("model")));
    }
}

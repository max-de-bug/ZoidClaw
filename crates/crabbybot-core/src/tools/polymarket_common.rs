//! Shared utilities for all Polymarket tools.
//!
//! Provides HTTP client construction (rustls + DNS overrides), authenticated
//! CLOB client builders, formatting helpers, and API constants.

use crate::config::PolymarketConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

// ── API Endpoints ──────────────────────────────────────────────────

pub const GAMMA_API_URL: &str = "https://gamma-api.polymarket.com";
pub const CLOB_API_URL: &str = "https://clob.polymarket.com";
pub const DATA_API_URL: &str = "https://data-api.polymarket.com";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Cloudflare IP for Polymarket domains — bypasses ISP DNS sinkholing.
const CLOUDFLARE_IP: &str = "104.18.34.205:443";

// ── HTTP Client ────────────────────────────────────────────────────

/// Build a `reqwest` client that uses rustls (bundled CA roots) and
/// DNS overrides to bypass ISP DNS sinkholing.
pub fn build_http_client() -> Result<reqwest::Client, reqwest::Error> {
    let cloudflare_ip = SocketAddr::from_str(CLOUDFLARE_IP).unwrap();

    reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(REQUEST_TIMEOUT)
        .user_agent("crabbybot/0.1")
        .resolve("gamma-api.polymarket.com", cloudflare_ip)
        .resolve("clob.polymarket.com", cloudflare_ip)
        .resolve("data-api.polymarket.com", cloudflare_ip)
        .build()
}

// ── Auth Helpers ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketCliConfig {
    pub private_key: String,
    pub chain_id: u64,
    #[serde(default = "default_signature_type")]
    pub signature_type: String,
}

fn default_signature_type() -> String {
    "proxy".to_string()
}

pub enum KeySource {
    EnvVar,
    BotConfig,
    ConfigFile,
    None,
}

impl KeySource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::EnvVar => "POLYMARKET_PRIVATE_KEY env var",
            Self::BotConfig => "crabbybot config.json",
            Self::ConfigFile => "~/.config/polymarket/config.json",
            Self::None => "not configured",
        }
    }
}

pub fn get_cli_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join("polymarket").join("config.json"))
}

pub fn load_cli_config() -> Option<PolymarketCliConfig> {
    let path = get_cli_config_path()?;
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_cli_config(key: &str, chain_id: u64, signature_type: &str) -> anyhow::Result<()> {
    let path = get_cli_config_path().ok_or_else(|| anyhow::anyhow!("No home dir found"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let config = PolymarketCliConfig {
        private_key: key.to_string(),
        chain_id,
        signature_type: signature_type.to_string(),
    };
    let json = serde_json::to_string_pretty(&config)?;
    fs::write(path, json)?;
    Ok(())
}

/// Resolves the wallet key and signature type. Priority: Env Var > Bot Config > CLI Config.
pub fn resolve_wallet_config(
    bot_config: &PolymarketConfig,
) -> (Option<String>, String, KeySource) {
    let mut sig_type = bot_config.signature_type.clone();
    
    // Check command line environment var
    if let Ok(key) = std::env::var("POLYMARKET_PRIVATE_KEY") {
        if !key.is_empty() {
            if let Ok(st) = std::env::var("POLYMARKET_SIGNATURE_TYPE") {
                if !st.is_empty() {
                    sig_type = st;
                }
            }
            return (Some(key), sig_type, KeySource::EnvVar);
        }
    }

    // Check bot config
    if let Some(key) = &bot_config.private_key {
        if !key.is_empty() {
            if let Ok(st) = std::env::var("POLYMARKET_SIGNATURE_TYPE") {
                if !st.is_empty() {
                    sig_type = st;
                }
            }
            return (Some(key.clone()), sig_type, KeySource::BotConfig);
        }
    }

    // Check CLI config
    if let Some(cli_cfg) = load_cli_config() {
        if let Ok(st) = std::env::var("POLYMARKET_SIGNATURE_TYPE") {
            if !st.is_empty() {
                sig_type = st;
            } else {
                sig_type = cli_cfg.signature_type;
            }
        } else {
            sig_type = cli_cfg.signature_type;
        }
        return (Some(cli_cfg.private_key), sig_type, KeySource::ConfigFile);
    }

    (None, sig_type, KeySource::None)
}

/// Returns `Some(key)` if the config has a private key, `None` otherwise.
pub fn private_key_from_config(config: &PolymarketConfig) -> Option<String> {
    resolve_wallet_config(config).0
}

/// Check if a wallet is configured and return a user-friendly error if not.
pub fn require_wallet(config: &PolymarketConfig) -> Result<String, String> {
    private_key_from_config(config).ok_or_else(|| {
        "❌ Polymarket wallet not configured. Run `polymarket_wallet_create`, \
         use `polymarket_wallet_import <key>`, or set `POLYMARKET_PRIVATE_KEY`."
            .to_string()
    })
}

pub async fn run_polymarket_cli(
    bot_config: &PolymarketConfig,
    args: &[&str],
) -> anyhow::Result<String> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["run", "-q", "-p", "polymarket-cli", "--"]);
    cmd.args(args);

    let (key_opt, sig_type_str, _source) = resolve_wallet_config(bot_config);
    if let Some(key) = key_opt {
        cmd.env("POLYMARKET_PRIVATE_KEY", key);
    }
    cmd.env("POLYMARKET_SIGNATURE_TYPE", sig_type_str);

    let output = cmd.output().map_err(|e| anyhow::anyhow!("Failed to run polymarket-cli: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(anyhow::anyhow!("CLI Error:\n{}\n{}", stdout, stderr));
    }

    Ok(stdout)
}

// ── Formatting Helpers ─────────────────────────────────────────────

/// Truncate a string to `max_len` characters, appending "…" if truncated.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

/// Format a USD value with appropriate suffix (B/M/K).
pub fn format_usd(value: Option<f64>) -> String {
    match value {
        Some(v) if v >= 1_000_000_000.0 => format!("${:.2}B", v / 1_000_000_000.0),
        Some(v) if v >= 1_000_000.0 => format!("${:.2}M", v / 1_000_000.0),
        Some(v) if v >= 1_000.0 => format!("${:.1}K", v / 1_000.0),
        Some(v) if v > 0.0 => format!("${:.2}", v),
        _ => "N/A".to_string(),
    }
}

/// Simple percent-encoding for query strings.
pub fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '&' => "%26".to_string(),
            '=' => "%3D".to_string(),
            '#' => "%23".to_string(),
            _ if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' => {
                c.to_string()
            }
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

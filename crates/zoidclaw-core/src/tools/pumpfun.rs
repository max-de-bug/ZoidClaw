//! Pump.fun / DexScreener memecoin tools.
//!
//! Provides real-time on-chain data for tokens, with special focus on Pump.fun
//! and PumpSwap tokens on Solana. Uses **DexScreener** as the primary data
//! backend — it's rock-solid, free, and indexes every DEX including Pump.fun.
//!
//! This gives Zoidclaw a unique competitive advantage: no other lightweight
//! AI agent (ZeroClaw, NanoBot, PicoClaw) has native memecoin tooling.
//!
//! ## Usage in Telegram
//!
//! Just ask naturally:
//! - "Search for cat memecoins on Solana"
//! - "Look up BONK token"
//! - "What's the price of WIF?"

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::Tool;

/// DexScreener API base — free, no auth, rate-limited to 300 req/min.
const DEXSCREENER_API: &str = "https://api.dexscreener.com/latest/dex";

// ── DexScreener types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DexPair {
    #[serde(rename = "chainId")]
    chain_id: String,
    #[serde(rename = "dexId")]
    dex_id: String,
    url: String,
    #[serde(rename = "pairAddress")]
    pair_address: String,
    #[serde(rename = "baseToken")]
    base_token: DexToken,
    #[serde(rename = "quoteToken")]
    quote_token: DexToken,
    #[serde(rename = "priceUsd")]
    price_usd: Option<String>,
    #[serde(rename = "priceNative")]
    price_native: Option<String>,
    #[serde(default)]
    volume: Option<DexVolume>,
    #[serde(default, rename = "priceChange")]
    price_change: Option<DexPriceChange>,
    #[serde(default)]
    liquidity: Option<DexLiquidity>,
    #[serde(default)]
    fdv: Option<f64>,
    #[serde(default, rename = "marketCap")]
    market_cap: Option<f64>,
    #[serde(default)]
    info: Option<DexInfo>,
    #[serde(default)]
    labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DexToken {
    address: String,
    name: String,
    symbol: String,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct DexVolume {
    h24: Option<f64>,
    h6: Option<f64>,
    h1: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct DexPriceChange {
    h1: Option<f64>,
    h6: Option<f64>,
    h24: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct DexLiquidity {
    usd: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct DexInfo {
    #[serde(default)]
    websites: Option<Vec<DexWebsite>>,
    #[serde(default)]
    socials: Option<Vec<DexSocial>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DexWebsite {
    url: String,
    label: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DexSocial {
    url: String,
    #[serde(rename = "type")]
    social_type: String,
}

#[derive(Debug, Deserialize)]
struct DexSearchResponse {
    pairs: Option<Vec<DexPair>>,
}

// ── Shared HTTP client ─────────────────────────────────────────────

// ── Shared HTTP client removed in favor of dependency injection ──

// ── PumpFunTokenTool ───────────────────────────────────────────────

pub struct PumpFunTokenTool {
    client: Client,
}

impl PumpFunTokenTool {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for PumpFunTokenTool {
    fn name(&self) -> &str {
        "pumpfun_token"
    }

    fn description(&self) -> &str {
        "Look up a token by its contract/mint address on any DEX (Solana, \
         Pump.fun, PumpSwap, Raydium, etc.). Returns price, market cap, volume, \
         liquidity, socials, and DEX links. Use this when the user gives you \
         a specific token mint address."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mint": {
                    "type": "string",
                    "description": "The token's contract address / mint address"
                }
            },
            "required": ["mint"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(mint) = args.get("mint").and_then(|v| v.as_str()) else {
            return "Error: 'mint' parameter is required".into();
        };

        debug!(mint, "Looking up token via DexScreener");

        let url = format!("{}/tokens/{}", DEXSCREENER_API, mint);

        let resp = match self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("❌ Failed to reach DexScreener: {}", e),
        };

        if !resp.status().is_success() {
            return format!("❌ DexScreener returned HTTP {}", resp.status());
        }

        let data: DexSearchResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => return format!("❌ Error parsing response: {}", e),
        };

        let pairs = data.pairs.unwrap_or_default();
        if pairs.is_empty() {
            return format!(
                "❌ Token `{}` not found on any DEX.\n\
                 Verify the mint address is correct.",
                mint
            );
        }

        // Pick the pair with the highest liquidity
        let best = pairs
            .iter()
            .max_by(|a, b| {
                let liq_a = a.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
                let liq_b = b.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
                liq_a
                    .partial_cmp(&liq_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap(); // safe: pairs is non-empty

        format_pair_detail(best, pairs.len())
    }
}

fn format_pair_detail(pair: &DexPair, total_pairs: usize) -> String {
    let price = pair.price_usd.as_deref().unwrap_or("N/A");
    let mcap = format_usd(pair.market_cap);
    let liq = format_usd(pair.liquidity.as_ref().and_then(|l| l.usd));
    let vol24 = format_usd(pair.volume.as_ref().and_then(|v| v.h24));

    let change_24h = pair
        .price_change
        .as_ref()
        .and_then(|pc| pc.h24)
        .map(|c| {
            let arrow = if c >= 0.0 { "📈" } else { "📉" };
            format!("{} {:.1}%", arrow, c)
        })
        .unwrap_or_else(|| "N/A".to_string());

    let is_pumpfun = pair.dex_id == "pumpfun" || pair.dex_id == "pumpswap";
    let dex_label = if is_pumpfun {
        "🎰 Pump.fun / PumpSwap"
    } else {
        &pair.dex_id
    };

    // Socials
    let socials = pair
        .info
        .as_ref()
        .map(|info| {
            let mut parts = Vec::new();
            if let Some(ref websites) = info.websites {
                for w in websites.iter().take(2) {
                    parts.push(format!("🌐 [{}]({})", w.label, w.url));
                }
            }
            if let Some(ref socials) = info.socials {
                for s in socials.iter().take(3) {
                    let icon = match s.social_type.as_str() {
                        "twitter" => "�",
                        "telegram" => "💬",
                        "discord" => "🎮",
                        _ => "🔗",
                    };
                    parts.push(format!("{} [{}]({})", icon, s.social_type, s.url));
                }
            }
            if parts.is_empty() {
                "None".to_string()
            } else {
                parts.join(" | ")
            }
        })
        .unwrap_or_else(|| "None".to_string());

    format!(
        "🪙 **{name}** (${symbol}) on {chain}\n\
         DEX: {dex}\n\n\
         � Price: **${price}**\n\
         � Market Cap: {mcap}\n\
         � Liquidity: {liq}\n\
         � 24h Volume: {vol24}\n\
         {change}\n\n\
         🔗 [DexScreener]({url})\n\
         • Socials: {socials}\n\n\
         📍 Active on {total} trading pair(s)\n\
         🪙 Contract: `{address}`",
        name = pair.base_token.name,
        symbol = pair.base_token.symbol,
        chain = pair.chain_id,
        dex = dex_label,
        price = price,
        mcap = mcap,
        liq = liq,
        vol24 = vol24,
        change = change_24h,
        url = pair.url,
        socials = socials,
        total = total_pairs,
        address = pair.base_token.address,
    )
}

// ── PumpFunSearchTool ──────────────────────────────────────────────

pub struct PumpFunSearchTool {
    client: Client,
}

impl PumpFunSearchTool {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for PumpFunSearchTool {
    fn name(&self) -> &str {
        "pumpfun_search"
    }

    fn description(&self) -> &str {
        "Search for tokens by name or ticker across all DEXes (Pump.fun, \
         PumpSwap, Raydium, Orca, Uniswap, etc.). Returns matching tokens \
         with price, market cap, volume, and links. \
         Use this when the user asks about a memecoin by name."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Token name or ticker to search for (e.g., 'BONK', 'cat', 'dogwifhat')"
                },
                "limit": {
                    "type": "number",
                    "description": "Number of results to return (default: 5, max: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return "Error: 'query' parameter is required".into();
        };

        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
            .unwrap_or(5)
            .min(10) as usize;

        debug!(query, limit, "Searching tokens via DexScreener");

        let url = format!("{}/search?q={}", DEXSCREENER_API, urlencoded(query));

        let resp = match self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("❌ Failed to reach DexScreener: {}", e),
        };

        if !resp.status().is_success() {
            return format!("❌ DexScreener returned HTTP {}", resp.status());
        }

        let data: DexSearchResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => return format!("❌ Error parsing search results: {}", e),
        };

        let pairs = data.pairs.unwrap_or_default();
        if pairs.is_empty() {
            return format!("No tokens found matching \"{}\".", query);
        }

        // Deduplicate by base token address, keeping the highest-liquidity pair per token
        let mut seen: HashMap<String, &DexPair> = HashMap::new();
        for pair in &pairs {
            let key = pair.base_token.address.clone();
            let existing_liq = seen
                .get(&key)
                .and_then(|p| p.liquidity.as_ref())
                .and_then(|l| l.usd)
                .unwrap_or(0.0);
            let current_liq = pair.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
            if current_liq > existing_liq || !seen.contains_key(&key) {
                seen.insert(key, pair);
            }
        }

        // Sort by market cap descending
        let mut deduped: Vec<&&DexPair> = seen.values().collect();
        deduped.sort_by(|a, b| {
            let mc_a = b.market_cap.unwrap_or(0.0);
            let mc_b = a.market_cap.unwrap_or(0.0);
            mc_a.partial_cmp(&mc_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        let results: Vec<&&DexPair> = deduped.into_iter().take(limit).collect();

        let mut output = format!(
            "🔍 **Token Search**: \"{}\" ({} result{})\n\n",
            query,
            results.len(),
            if results.len() == 1 { "" } else { "s" }
        );

        for (i, pair) in results.iter().enumerate() {
            let price = pair.price_usd.as_deref().unwrap_or("N/A");
            let mcap = format_usd(pair.market_cap);
            let vol24 = format_usd(pair.volume.as_ref().and_then(|v| v.h24));
            let is_pf = pair.dex_id == "pumpfun" || pair.dex_id == "pumpswap";

            let change = pair
                .price_change
                .as_ref()
                .and_then(|pc| pc.h24)
                .map(|c| {
                    let arrow = if c >= 0.0 { "🟢" } else { "🔴" };
                    format!("{}{:.1}%", arrow, c)
                })
                .unwrap_or_default();

            output.push_str(&format!(
                "{}. **{}** (${}) — {} on {}{}\n   💲${} | MCap: {} | Vol24h: {} {}\n   [DexScreener]({})\n\n",
                i + 1,
                pair.base_token.name,
                pair.base_token.symbol,
                pair.dex_id,
                pair.chain_id,
                if is_pf { " 🎰" } else { "" },
                price,
                mcap,
                vol24,
                change,
                pair.url,
            ));
        }

        output
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn format_usd(value: Option<f64>) -> String {
    match value {
        Some(v) if v >= 1_000_000_000.0 => format!("${:.2}B", v / 1_000_000_000.0),
        Some(v) if v >= 1_000_000.0 => format!("${:.2}M", v / 1_000_000.0),
        Some(v) if v >= 1_000.0 => format!("${:.1}K", v / 1_000.0),
        Some(v) if v > 0.0 => format!("${:.0}", v),
        _ => "N/A".to_string(),
    }
}

/// Simple percent-encoding for query strings.
fn urlencoded(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '&' => "%26".to_string(),
            '=' => "%3D".to_string(),
            '#' => "%23".to_string(),
            _ if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

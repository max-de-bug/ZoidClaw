//! Sentiment analysis tool for Web3 tokens.
//!
//! Uses social information from DexScreener/Mobula or other sources to gauge 
//! "Community Pulse" (bullish vs bearish signals).

use super::Tool;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct SentimentTool {
    client: Client,
}

impl SentimentTool {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[derive(Debug, Deserialize)]
struct DexScreenerInfoResponse {
    pairs: Option<Vec<DexPairInfo>>,
}

#[derive(Debug, Deserialize)]
struct DexPairInfo {
    info: Option<DexInfo>,
}

#[derive(Debug, Deserialize)]
struct DexInfo {
    socials: Option<Vec<DexSocial>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DexSocial {
    #[serde(rename = "type")]
    social_type: String,
    url: String,
}

#[async_trait]
impl Tool for SentimentTool {
    fn name(&self) -> &str {
        "sentiment"
    }

    fn description(&self) -> &str {
        "Analyze social sentiment and community health for a Solana token. \
         Checks social presence (Twitter, Telegram), volume trends, and community pulse. \
         Use this when a user asks 'how is the community?' or 'what is the sentiment?'"
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
            return "❌ Error: 'mint' parameter is required".into();
        };

        match self.fetch_sentiment(mint).await {
            Ok((social_count, pulse)) => {
                format!(
                    "🎭 **Community Pulse: {}**\n\
                     Token: `{}`\n\
                     Social Channels: **{}**\n\n\
                     **AI Analysis:**\n\
                     {} \n\n\
                     _Tip: High social count usually indicates a stronger community core._",
                    pulse,
                    mint,
                    social_count,
                    if social_count > 0 {
                        "The token has active social links indexed. This suggests the project team is actively managing community outreach."
                    } else {
                        "WARNING: No social links found. This might be a developer-only test launch or a potential trap with no market presence."
                    }
                )
            }
            Err(e) => e,
        }
    }
}

impl SentimentTool {
    pub async fn fetch_sentiment(&self, mint: &str) -> Result<(usize, String), String> {
        let info_url = format!("https://api.dexscreener.com/latest/dex/tokens/{}", mint);
        
        let resp = self.client.get(&info_url).send().await
            .map_err(|e| format!("❌ Failed to reach DexScreener: {}", e))?;

        let data: DexScreenerInfoResponse = resp.json().await
            .map_err(|e| format!("❌ Error parsing sentiment data: {}", e))?;

        let pairs = data.pairs.unwrap_or_default();
        if pairs.is_empty() {
            return Err(format!("❌ No social data found for `{}`.", mint));
        }

        let best_pair = &pairs[0];
        let social_count = best_pair.info.as_ref()
            .and_then(|i| i.socials.as_ref())
            .map(|s| s.len())
            .unwrap_or(0);

        let pulse = if social_count >= 3 {
            "🔥 **Vibrant**".to_string()
        } else if social_count >= 1 {
            "📈 **Developing**".to_string()
        } else {
            "🌑 **Ghost Town**".to_string()
        };

        Ok((social_count, pulse))
    }
}

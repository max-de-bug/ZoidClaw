//! Alpha Summary tool: Orchestrates multiple sub-tools to provide a "Signal Score".
//!
//! Synthesizes data from RugCheck (Safety) and Sentiment (Social) concurrently.

use super::rugcheck::{RugCheckTool, RugcheckReport};
use super::sentiment::SentimentTool;
use super::Tool;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::try_join;

pub struct AlphaSummaryTool {
    rugcheck: RugCheckTool,
    sentiment: SentimentTool,
}

impl AlphaSummaryTool {
    pub fn new(client: Client) -> Self {
        Self {
            rugcheck: RugCheckTool::new(client.clone()),
            sentiment: SentimentTool::new(client),
        }
    }
}

#[async_trait]
impl Tool for AlphaSummaryTool {
    fn name(&self) -> &str {
        "alpha_summary"
    }

    fn description(&self) -> &str {
        "Get a comprehensive 'Alpha Signal' for a token. Synthesizes RugCheck risk, \
         social sentiment, and community activity into a single decision-support report. \
         Use this when a user asks 'is this token good?', 'give me alpha on [CA]', or 'check [CA]'."
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

        // Orchestration: Fetch both concurrently
        let rug_fut = self.rugcheck.fetch_report(mint);
        let sent_fut = self.sentiment.fetch_sentiment(mint);

        match try_join!(rug_fut, sent_fut) {
            Ok((rug_report, (social_count, pulse))) => {
                format_alpha_report(mint, &rug_report, social_count, &pulse)
            }
            Err(e) => {
                // If one fails, try to return what we have or a specific error
                format!("❌ Alpha Summary failed partially or fully: {}", e)
            }
        }
    }
}

fn format_alpha_report(
    mint: &str,
    rug: &RugcheckReport,
    social_count: usize,
    pulse: &str,
) -> String {
    // Scoring Logic (Heuristic)
    let rug_score_norm = (rug.score as f32 / 10000.0).min(1.0); // 0 (safe) to 1 (danger)
    let social_norm = (social_count as f32 / 5.0).min(1.0); // 0 (none) to 1 (vibrant)

    // Alpha Score: Higher is better.
    // Base 50 + (1 - RugScore)*50 + (Social)*20? No, let's keep it simple.
    let alpha_score = ((1.0 - rug_score_norm) * 70.0 + (social_norm * 30.0)) as i32;

    let signal = if alpha_score >= 80 {
        "🚀 **STRONG BUY SIGNAL**"
    } else if alpha_score >= 60 {
        "✅ **PROCEED WITH CAUTION**"
    } else if alpha_score >= 40 {
        "⚠️ **SPECULATIVE / HIGH RISK**"
    } else {
        "🛑 **AVOID / RUG RISK**"
    };

    let safety_icon = if rug.score < 2000 {
        "🟢"
    } else if rug.score < 5000 {
        "🟡"
    } else {
        "🔴"
    };

    format!(
        "🌟 **Alpha Core Summary: {}**\n\
         Token: **{}** (`{}`)\n\
         Mint: `{}`\n\n\
         📊 **Signal Metrics:**\n\
         • Alpha Score: **{} / 100**\n\
         • Safety: {} **{}** (RugCheck: {})\n\
         • Pulse: {}\n\n\
         🔎 **AI Verdict:**\n\
         {}\n\n\
         🔗 [DexScreener](https://dexscreener.com/solana/{}) | [RugCheck](https://rugcheck.xyz/tokens/{})",
        signal,
        rug.file_meta.name, rug.file_meta.symbol,
        mint,
        alpha_score,
        safety_icon,
        if rug.score < 2000 { "Safe" } else if rug.score < 5000 { "Warning" } else { "Danger" },
        rug.score,
        pulse,
        generate_verdict(alpha_score, rug.score, social_count),
        mint,
        mint
    )
}

fn generate_verdict(score: i32, rug_score: i32, socials: usize) -> String {
    if score >= 80 {
        "This token shows a rare combination of high social proof and clean contract audits. The risk-to-reward ratio looks optimal for a momentum play.".to_string()
    } else if rug_score > 5000 {
        "CRITICAL: RugCheck detected high-risk factors. Despite any social hype, the contract mechanics are dangerous. Engagement is not recommended.".to_string()
    } else if socials == 0 {
        "This is likely a 'stealth' or 'bot' launch. While the contract might be safe, there is zero social evidence of a community. High slippage and exit liquidity risk.".to_string()
    } else {
        "A standard speculative play. Ensure you aren't over-allocated. The community is present but the 'Alpha' isn't overwhelming yet.".to_string()
    }
}

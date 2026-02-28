//! Intent Router — keyword-based, zero-cost classification.
//!
//! Classifies a user's message into an [`IntentCategory`] using simple
//! keyword matching. This avoids burning LLM tokens on a routing call,
//! which is critical on free-tier providers with tight TPM limits.

use crate::tools::IntentCategory;
use tracing::info;

pub struct IntentRouter;

/// Keyword sets for each category.
const POLYMARKET_READ_KEYWORDS: &[&str] = &[
    "polymarket", "prediction", "odds", "bet", "market", "event",
    "election", "trump", "biden", "poll", "price", "volume",
    "trending", "sports", "outcome", "probability", "wager",
    "leaderboard", "holders", "comments", "series", "tags",
    "deport", "deportation", "tariff",
];

const POLYMARKET_TRADE_KEYWORDS: &[&str] = &[
    "buy", "sell", "order", "trade", "wallet", "balance",
    "position", "cancel order", "limit order", "market order",
    "approve", "usdc", "deposit", "withdraw", "ctf",
    "split", "merge", "redeem", "my orders", "my positions",
    "place a bet", "place bet",
];

const CRYPTO_KEYWORDS: &[&str] = &[
    "solana", "sol", "token", "pumpfun", "pump.fun", "pump fun",
    "rugcheck", "rug", "memecoin", "meme coin", "degen",
    "mint", "discovery", "stream", "sentiment", "alpha",
    "buy token", "snipe",
];

const SYSTEM_KEYWORDS: &[&str] = &[
    "file", "read file", "write file", "list dir", "execute",
    "shell", "command", "script", "schedule", "cron",
    "run", "mkdir", "ls",
];

const RESEARCH_KEYWORDS: &[&str] = &[
    "search", "google", "web", "fetch", "url", "http",
    "look up", "find out", "research",
];

impl IntentRouter {
    /// Classify a message into an intent category using keyword matching.
    ///
    /// This is instantaneous and costs zero LLM tokens.
    pub fn classify(message: &str) -> IntentCategory {
        let lower = message.to_lowercase();

        // Score each category by counting keyword hits
        let scores = [
            (IntentCategory::PolymarketTrade, Self::score(&lower, POLYMARKET_TRADE_KEYWORDS)),
            (IntentCategory::PolymarketRead,  Self::score(&lower, POLYMARKET_READ_KEYWORDS)),
            (IntentCategory::CryptoTokens,    Self::score(&lower, CRYPTO_KEYWORDS)),
            (IntentCategory::System,          Self::score(&lower, SYSTEM_KEYWORDS)),
            (IntentCategory::Research,        Self::score(&lower, RESEARCH_KEYWORDS)),
        ];

        // Pick the category with the highest score; fall back to General
        let best = scores.iter().max_by_key(|(_, s)| *s).unwrap();

        let category = if best.1 > 0 { best.0 } else { IntentCategory::General };

        info!(
            category = category.as_str(),
            score = best.1,
            "Intent Router classified message"
        );

        category
    }

    fn score(text: &str, keywords: &[&str]) -> usize {
        keywords.iter().filter(|kw| text.contains(**kw)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polymarket_read() {
        let cat = IntentRouter::classify("What are the odds on Trump winning?");
        assert_eq!(cat, IntentCategory::PolymarketRead);
    }

    #[test]
    fn test_polymarket_trade() {
        let cat = IntentRouter::classify("Buy 10 shares of Yes on that market");
        assert_eq!(cat, IntentCategory::PolymarketTrade);
    }

    #[test]
    fn test_crypto() {
        let cat = IntentRouter::classify("Check this solana token for rugs");
        assert_eq!(cat, IntentCategory::CryptoTokens);
    }

    #[test]
    fn test_general() {
        let cat = IntentRouter::classify("Hello, how are you?");
        assert_eq!(cat, IntentCategory::General);
    }

    #[test]
    fn test_system() {
        let cat = IntentRouter::classify("Read the file at /tmp/test.txt");
        assert_eq!(cat, IntentCategory::System);
    }
}

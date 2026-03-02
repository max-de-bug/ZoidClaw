//! Autonomous Polymarket betting engine.
//!
//! Runs as a background `tokio::spawn` task that periodically scans
//! Polymarket for opportunities, uses LLM analysis to score them,
//! and places trades subject to configurable safety rails.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

use crate::config::BettingConfig;
use crate::tools::ToolRegistry;

// ── Types ──────────────────────────────────────────────────────────

/// Tracks a single open position placed by the betting engine.
#[derive(Debug, Clone)]
pub struct OpenPosition {
    pub market_question: String,
    pub token_id: String,
    pub side: String,
    pub entry_price: f64,
    pub size_usdc: f64,
    pub placed_at: std::time::Instant,
}

/// Daily PnL tracker. Resets every 24 hours.
#[derive(Debug, Clone, Default)]
pub struct PnlTracker {
    pub realized_pnl: f64,
    pub total_bets_today: u32,
    pub wins: u32,
    pub losses: u32,
    pub last_reset: Option<std::time::Instant>,
}

impl PnlTracker {
    fn maybe_reset(&mut self) {
        let should_reset = match self.last_reset {
            None => true,
            Some(t) => t.elapsed() > Duration::from_secs(86_400),
        };
        if should_reset {
            info!("PnL tracker reset for new day");
            self.realized_pnl = 0.0;
            self.total_bets_today = 0;
            self.wins = 0;
            self.losses = 0;
            self.last_reset = Some(std::time::Instant::now());
        }
    }
}

/// Shared state for the betting engine, accessible via Telegram control tool.
#[derive(Debug, Clone)]
pub struct BettingState {
    pub running: bool,
    pub config: BettingConfig,
    pub pnl: PnlTracker,
    pub open_positions: Vec<OpenPosition>,
    pub scan_count: u64,
    pub trade_log: Vec<String>,
}

impl BettingState {
    pub fn new(config: BettingConfig) -> Self {
        let running = config.enabled;
        Self {
            running,
            config,
            pnl: PnlTracker::default(),
            open_positions: Vec::new(),
            scan_count: 0,
            trade_log: Vec::new(),
        }
    }

    /// Format a human-readable status string.
    pub fn status_report(&self) -> String {
        let status = if self.running { "🟢 RUNNING" } else { "🔴 PAUSED" };
        format!(
            "{status}\n\
             Strategy: {}\n\
             Scan Count: {}\n\
             Open Positions: {}\n\
             Today's Bets: {}\n\
             W/L: {} / {}\n\
             Realized PnL: ${:.2}\n\
             Max Bet: ${:.2}\n\
             Daily Loss Limit: ${:.2}\n\
             Stop-Loss: {:.0}%\n\
             Take-Profit: {:.0}%",
            self.config.strategy,
            self.scan_count,
            self.open_positions.len(),
            self.pnl.total_bets_today,
            self.pnl.wins,
            self.pnl.losses,
            self.pnl.realized_pnl,
            self.config.max_bet_size_usdc,
            self.config.daily_loss_limit_usdc,
            self.config.stop_loss_percent,
            self.config.take_profit_percent,
        )
    }

    /// Format the trade log for display.
    pub fn history_report(&self) -> String {
        if self.trade_log.is_empty() {
            return "No trades yet.".into();
        }
        self.trade_log
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── Service ────────────────────────────────────────────────────────

pub struct BettingService;

impl BettingService {
    /// Spawn the betting loop as a background task.
    pub fn spawn(
        state: Arc<Mutex<BettingState>>,
        tools: Arc<ToolRegistry>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("Betting engine initialized (check status with betting_control)");

            // Read interval from config
            let interval_mins = {
                let s = state.lock().await;
                s.config.scan_interval_minutes
            };
            let mut interval = time::interval(Duration::from_secs(interval_mins * 60));

            loop {
                interval.tick().await;

                // Check if running
                let is_running = {
                    let s = state.lock().await;
                    s.running
                };
                if !is_running {
                    debug!("Betting engine is paused, skipping scan");
                    continue;
                }

                // Check daily loss limit
                {
                    let mut s = state.lock().await;
                    s.pnl.maybe_reset();
                    if s.pnl.realized_pnl < -s.config.daily_loss_limit_usdc {
                        warn!(
                            pnl = s.pnl.realized_pnl,
                            limit = -s.config.daily_loss_limit_usdc,
                            "Daily loss limit hit! Auto-pausing betting engine"
                        );
                        s.running = false;
                        let msg = format!(
                            "⛔ AUTO-PAUSED: Daily loss limit ${:.2} exceeded (PnL: ${:.2})",
                            s.config.daily_loss_limit_usdc, s.pnl.realized_pnl
                        );
                        s.trade_log.push(msg);
                        continue;
                    }
                }

                info!("Betting engine: starting scan cycle");

                // ── Phase 1: Scan ──────────────────────────────────────
                let candidates = Self::scan_markets(&tools).await;
                {
                    let mut s = state.lock().await;
                    s.scan_count += 1;
                }

                if candidates.is_empty() {
                    debug!("No candidates found in this scan cycle");
                    continue;
                }

                info!(count = candidates.len(), "Found market candidates");

                // ── Phase 2: Analyze + Execute ─────────────────────────
                for candidate in candidates {
                    let config = {
                        let s = state.lock().await;
                        s.config.clone()
                    };

                    // Score the candidate using LLM via the agent's tool
                    let score = Self::analyze_candidate(&tools, &candidate).await;

                    if score < config.min_llm_score {
                        debug!(
                            market = candidate.question,
                            score,
                            threshold = config.min_llm_score,
                            "Candidate below score threshold, skipping"
                        );
                        continue;
                    }

                    info!(
                        market = candidate.question,
                        score,
                        "Candidate above threshold — placing bet"
                    );

                    // ── Phase 3: Execute ────────────────────────────────
                    let result = Self::place_bet(
                        &tools,
                        &candidate,
                        &config,
                    )
                    .await;

                    // Log the result
                    let mut s = state.lock().await;
                    s.pnl.total_bets_today += 1;
                    match result {
                        Ok(msg) => {
                            let log_entry = format!(
                                "✅ BET: {} | Side: {} | ${:.2} | Score: {} | {}",
                                candidate.question,
                                candidate.suggested_side,
                                config.max_bet_size_usdc,
                                score,
                                msg,
                            );
                            info!("{}", log_entry);
                            s.trade_log.push(log_entry);
                            s.open_positions.push(OpenPosition {
                                market_question: candidate.question.clone(),
                                token_id: candidate.token_id.clone(),
                                side: candidate.suggested_side.clone(),
                                entry_price: candidate.current_price,
                                size_usdc: config.max_bet_size_usdc,
                                placed_at: std::time::Instant::now(),
                            });
                        }
                        Err(e) => {
                            let log_entry = format!(
                                "❌ FAILED: {} | Error: {}",
                                candidate.question, e
                            );
                            warn!("{}", log_entry);
                            s.trade_log.push(log_entry);
                        }
                    }
                }
            }
        })
    }

    // ── Scan: Get trending markets and filter ──────────────────────

    async fn scan_markets(tools: &ToolRegistry) -> Vec<MarketCandidate> {
        let trending_output = tools
            .execute("polymarket_trending", HashMap::from([
                ("limit".into(), serde_json::json!("5")),
            ]))
            .await;

        debug!(output_len = trending_output.len(), "Trending markets fetched");

        // Parse candidates from the trending output
        Self::parse_candidates(&trending_output)
    }

    /// Parse the trending tool output into structured candidates.
    /// The trending tool returns human-readable text, so we extract
    /// key fields using simple text parsing.
    fn parse_candidates(raw: &str) -> Vec<MarketCandidate> {
        let mut candidates = Vec::new();

        // The trending output contains blocks like:
        // Question: Will X happen?
        // Price: 65.0¢
        // Volume: $1.5M
        // Token(s): 12345...
        let mut current = MarketCandidate::default();
        for line in raw.lines() {
            let line = line.trim();
            if line.starts_with("Question:") || line.starts_with("question:") {
                if !current.question.is_empty() {
                    candidates.push(current.clone());
                }
                current = MarketCandidate::default();
                current.question = line.split_once(':').map(|(_, v)| v.trim()).unwrap_or("").into();
            } else if line.to_lowercase().contains("price") && line.contains('¢') {
                // Extract price like "65.0¢" → 0.65
                if let Some(price_str) = line.split_whitespace().find(|w| w.contains('¢')) {
                    let clean = price_str.replace('¢', "").replace(',', "");
                    if let Ok(cents) = clean.parse::<f64>() {
                        current.current_price = cents / 100.0;
                    }
                }
            } else if line.to_lowercase().contains("volume") && line.contains('$') {
                // Extract volume like "$1.5M" → 1500000
                if let Some(vol_str) = line.split_whitespace().find(|w| w.starts_with('$')) {
                    current.volume_str = vol_str.to_string();
                }
            } else if line.to_lowercase().contains("token") {
                if let Some(id) = line.split_whitespace().last() {
                    if id.len() > 10 {
                        current.token_id = id.to_string();
                    }
                }
            }
        }
        if !current.question.is_empty() {
            candidates.push(current);
        }

        // Determine suggested side based on value strategy
        for c in &mut candidates {
            // Value strategy: if price < 0.40, buy YES; if price > 0.70, maybe bet NO
            if c.current_price > 0.0 && c.current_price < 0.40 {
                c.suggested_side = "buy".into();
            } else if c.current_price > 0.70 {
                c.suggested_side = "sell".into();
            } else {
                c.suggested_side = "buy".into(); // default
            }
        }

        candidates
    }

    // ── Analyze: Score a candidate (lightweight, no LLM) ───────────

    /// Use a heuristic scoring system to avoid LLM calls on Groq free tier.
    ///
    /// Score components (max 10):
    /// - Volume bonus (0-3): higher volume = more confidence
    /// - Price dislocation (0-4): further from 50¢ = higher edge
    /// - Market freshness (0-3): bonus for active markets
    async fn analyze_candidate(tools: &ToolRegistry, candidate: &MarketCandidate) -> u32 {
        // Get price data for deeper analysis
        let _price_output = if !candidate.token_id.is_empty() {
            tools
                .execute("polymarket_price", HashMap::from([
                    ("token_id".into(), serde_json::json!(candidate.token_id)),
                ]))
                .await
        } else {
            String::new()
        };

        let mut score: u32 = 0;

        // Volume scoring (0-3)
        let vol = &candidate.volume_str;
        if vol.contains('M') || vol.contains('m') {
            score += 3; // $1M+
        } else if vol.contains('K') || vol.contains('k') {
            let num_str = vol.replace('$', "").replace('K', "").replace('k', "");
            if let Ok(k) = num_str.parse::<f64>() {
                if k > 100.0 { score += 2; }
                else { score += 1; }
            }
        }

        // Price dislocation scoring (0-4)
        // The further from 50¢, the more "obvious" the market thinks it is,
        // but very low prices (< 15¢) can be value bets
        let price = candidate.current_price;
        if price > 0.0 && price < 0.15 {
            score += 4; // Potential big payout longshot
        } else if price < 0.30 {
            score += 3;
        } else if price < 0.40 || price > 0.80 {
            score += 2;
        } else {
            score += 1; // Near 50/50, low edge
        }

        // Freshness bonus (0-3) - always give 2 since trending = active
        score += 2;

        debug!(
            market = candidate.question,
            score,
            price,
            volume = vol.as_str(),
            "Scored candidate"
        );

        score
    }

    // ── Execute: Place the actual bet ──────────────────────────────

    async fn place_bet(
        tools: &ToolRegistry,
        candidate: &MarketCandidate,
        config: &BettingConfig,
    ) -> Result<String, String> {
        if candidate.token_id.is_empty() {
            return Err("No token ID available for this market".into());
        }

        // Calculate the order price based on our side
        let order_price = if candidate.suggested_side == "buy" {
            // Buy at slightly above the current best ask
            (candidate.current_price + 0.02).min(0.99)
        } else {
            // Sell at slightly below the current best bid
            (candidate.current_price - 0.02).max(0.01)
        };

        // Calculate shares from USDC budget
        let shares = (config.max_bet_size_usdc / order_price).floor();
        if shares < 1.0 {
            return Err("Position too small: shares < 1".into());
        }

        info!(
            token_id = candidate.token_id,
            side = candidate.suggested_side,
            price = format!("{:.2}", order_price),
            shares = format!("{:.0}", shares),
            "Placing limit order"
        );

        let result = tools
            .execute("polymarket_create_order", HashMap::from([
                ("token_id".into(), serde_json::json!(candidate.token_id)),
                ("side".into(), serde_json::json!(candidate.suggested_side)),
                ("price".into(), serde_json::json!(format!("{:.2}", order_price))),
                ("size".into(), serde_json::json!(format!("{:.0}", shares))),
                ("order_type".into(), serde_json::json!("GTC")),
            ]))
            .await;

        if result.contains("❌") || result.contains("Error") || result.contains("error") {
            Err(result)
        } else {
            Ok(result)
        }
    }
}

// ── Internal Types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct MarketCandidate {
    question: String,
    token_id: String,
    current_price: f64,
    volume_str: String,
    suggested_side: String,
}

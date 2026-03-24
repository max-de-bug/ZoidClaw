//! 🦀 CrabbyBot CLI — interactive chat, onboarding, and status commands.
//!
//! Usage:
//!   CrabbyBot chat          — Start an interactive chat session
//!   CrabbyBot onboard       — Create a default configuration
//!   CrabbyBot status        — Show current configuration and health
//!   CrabbyBot cron list      — List scheduled jobs
//!   CrabbyBot sessions       — List conversation sessions

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crabbybot_core::agent::{AgentConfig, AgentLoop};
use crabbybot_core::bus::MessageBus;
use crabbybot_core::config::Config;
use crabbybot_core::cron::{CronService, Schedule};
#[cfg(feature = "discord")]
use crabbybot_core::gateway::channels::discord::DiscordTransport;
#[cfg(feature = "telegram")]
use crabbybot_core::gateway::channels::telegram::TelegramTransport;
use crabbybot_core::gateway::AgentBridge;
use tracing::warn;
use crabbybot_core::provider::openai::OpenAiProvider;
use crabbybot_core::provider::LlmProvider;
use crabbybot_core::session::SessionManager;
use crabbybot_core::tools::alpha_summary::AlphaSummaryTool;
use crabbybot_core::tools::filesystem::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
use crabbybot_core::tools::polymarket::{
    PolymarketMarketTool, PolymarketSearchTool, PolymarketTrendingTool,
};
use crabbybot_core::tools::polymarket_approve::PolymarketApproveTool;
use crabbybot_core::tools::polymarket_bridge::PolymarketBridgeTool;
use crabbybot_core::tools::polymarket_comments::PolymarketCommentsTool;
use crabbybot_core::tools::polymarket_ctf::{
    PolymarketCtfMergeTool, PolymarketCtfRedeemTool, PolymarketCtfSplitTool,
};
use crabbybot_core::tools::polymarket_data::{
    PolymarketActivityTool, PolymarketBuilderLeaderboardTool, PolymarketClosedPositionsTool,
    PolymarketHoldersTool, PolymarketLeaderboardTool, PolymarketOpenInterestTool,
    PolymarketPositionsTool, PolymarketTradesTool, PolymarketVolumeTool,
};
use crabbybot_core::tools::polymarket_events::{PolymarketEventDetailTool, PolymarketEventsTool};
use crabbybot_core::tools::polymarket_orderbook::{
    PolymarketClobMarketTool, PolymarketLastTradeTool, PolymarketOrderbookTool,
    PolymarketTickSizeTool,
};
use crabbybot_core::tools::polymarket_orders::{
    PolymarketAccountStatusTool, PolymarketApiKeysTool, PolymarketBalanceTool,
    PolymarketCancelOrderTool, PolymarketMyOrdersTool, PolymarketNotificationsTool,
    PolymarketRewardsTool,
};
use crabbybot_core::tools::polymarket_prices::{PolymarketPriceHistoryTool, PolymarketPriceTool};
use crabbybot_core::tools::polymarket_profiles::PolymarketProfileTool;
use crabbybot_core::tools::polymarket_series::PolymarketSeriesTool;
use crabbybot_core::tools::polymarket_sports::PolymarketSportsTool;
use crabbybot_core::tools::polymarket_status::PolymarketStatusTool;
use crabbybot_core::tools::polymarket_stream::PolymarketStreamTool;
use crabbybot_core::tools::polymarket_tags::PolymarketTagsTool;
use crabbybot_core::tools::polymarket_trade::{
    PolymarketCreateOrderTool, PolymarketMarketOrderTool,
};
use crabbybot_core::tools::polymarket_wallet::{
    PolymarketWalletCreateTool, PolymarketWalletImportTool, PolymarketWalletTool,
};
use crabbybot_core::tools::rugcheck::RugCheckTool;
use crabbybot_core::tools::schedule::{CancelScheduleTool, ListSchedulesTool, ScheduleTaskTool};
use crabbybot_core::tools::sentiment::SentimentTool;
use crabbybot_core::tools::shell::ExecTool;
use crabbybot_core::tools::solana::{
    SolanaBalanceTool, SolanaTokenBalancesTool, SolanaTransactionsTool,
};
use crabbybot_core::tools::web::{WebFetchTool, WebSearchTool};
use crabbybot_core::tools::betting_control::BettingControlTool;
use crabbybot_core::tools::prediction::{GraphQueryTool, PredictTool, SimulateTool};
use crabbybot_core::tools::prediction::tool_predict::PredictionState;
use crabbybot_core::tools::ToolRegistry;
use crabbybot_core::service::betting::{BettingService, BettingState};

#[derive(Parser)]
#[command(
    name = "CrabbyBot",
    version,
    about = "An ultra-lightweight personal AI assistant",
    long_about = "🦀 CrabbyBot — a blazing-fast AI assistant written in Rust.\n\nZero runtime dependencies. Single binary. Direct LLM API access."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session
    Chat {
        /// Session name (default: "default")
        #[arg(short, long, default_value = "default")]
        session: String,

        /// Model to use (overrides config)
        #[arg(short, long)]
        model: Option<String>,
    },

    /// Create or reset the default configuration
    Onboard,

    /// Show configuration status and health
    Status,

    /// Manage scheduled jobs
    Cron {
        #[command(subcommand)]
        action: CronCommands,
    },

    /// Start the bot in background mode (Telegram/Discord)
    Bot,

    /// Manage conversation sessions
    Sessions {
        #[command(subcommand)]
        action: Option<SessionCommands>,
    },
}

#[derive(Subcommand)]
enum CronCommands {
    /// List all scheduled jobs
    List,
    /// Add a new job
    Add {
        /// Job name
        #[arg(short, long)]
        name: String,
        /// Cron expression (e.g., "0 9 * * *")
        #[arg(short, long)]
        schedule: String,
        /// Message/prompt to execute
        #[arg(short, long)]
        message: String,
    },
    /// Remove a job
    Remove {
        /// Job ID
        id: String,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// List all sessions
    List,
    /// Delete a session
    Delete {
        /// Session key
        key: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Chat { session, model }) => cmd_chat(&session, model.as_deref()).await?,
        Some(Commands::Bot) => cmd_bot().await?,
        Some(Commands::Onboard) => cmd_onboard()?,
        Some(Commands::Status) => cmd_status()?,
        Some(Commands::Cron { action }) => cmd_cron(action)?,
        Some(Commands::Sessions { action }) => cmd_sessions(action)?,
        None => cmd_chat("default", None).await?,
    }

    Ok(())
}

use crabbybot_core::tools::IntentCategory;

// ── Shared Setup ────────────────────────────────────────────────────

/// Shared helper that loads config, validates it, and builds a fully
/// wired `AgentLoop` with providers and tools.
///
/// Returns `(agent, config, workspace_path)` so both `cmd_chat` and
/// `cmd_bot` can avoid duplicating this boilerplate.
fn validate_config(config: &Config) -> Result<()> {
    if let Err(errors) = config.validate() {
        let is_tg_enabled = config.channels.telegram.as_ref().is_some_and(|c| c.enabled && !c.token.is_empty());
        let is_dc_enabled = config.channels.discord.as_ref().is_some_and(|c| c.enabled && !c.token.is_empty());

        if is_tg_enabled || is_dc_enabled {
            warn!("Configuration has errors, but starting in setup mode since a channel is enabled:");
            for e in &errors {
                warn!("  • {}", e);
            }
            return Ok(());
        }

        eprintln!("\n  \x1b[31m❌ Configuration errors:\x1b[0m");
        for e in &errors {
            eprintln!("     • {}", e);
        }
        eprintln!();
        anyhow::bail!("Fix the above {} error(s) in config.json", errors.len());
    }
    Ok(())
}

fn setup_agent(
    config: &Config,
    model_override: Option<&str>,
    cron: Option<Arc<tokio::sync::Mutex<CronService>>>,
    bus: Arc<MessageBus>,
    default_channel: &str,
    default_chat_id: &str,
    betting_state: Option<Arc<tokio::sync::Mutex<BettingState>>>,
) -> Result<(AgentLoop, PathBuf, Arc<ToolRegistry>)> {
    let model = model_override
        .unwrap_or(&config.agents.defaults.model)
        .to_string();

    // Resolve providers
    let active_providers = config.providers.find_all_active();
    
    let provider: Box<dyn LlmProvider> = if active_providers.is_empty() {
        warn!("No active LLM providers. Bot will start in limited setup mode.");
        Box::new(crabbybot_core::provider::NoopProvider { model: model.clone() })
    } else {
        let client = reqwest::Client::new();
        let mut inner_providers = Vec::new();
        for (name, entry) in active_providers {
            let p_model = entry.model.as_deref().unwrap_or(&model);
            
            let api_key = crabbybot_core::vault::decrypt(&entry.api_key).unwrap_or_else(|e| {
                tracing::warn!("Failed to decrypt API key for provider {}: {}", name, e);
                entry.api_key.clone()
            });

            let p = OpenAiProvider::new(
                name,
                &api_key,
                entry.api_base.as_deref(),
                p_model,
                client.clone(),
            );
            inner_providers.push((name.to_string(), Box::new(p) as Box<dyn LlmProvider>));
        }
        Box::new(crabbybot_core::provider::FallbackProvider::new(inner_providers))
    };

    let provider: Arc<tokio::sync::Mutex<Box<dyn LlmProvider>>> =
        Arc::new(tokio::sync::Mutex::new(provider));

    let client = reqwest::Client::new();

    // Set up tools
    let workspace = config.workspace_path();
    let restrict = config.tools.restrict_to_workspace;
    let mut tools = ToolRegistry::new();

    tools.register(Box::new(ReadFileTool::new(workspace.clone(), restrict)), IntentCategory::System);
    tools.register(Box::new(WriteFileTool::new(workspace.clone(), restrict)), IntentCategory::System);
    tools.register(Box::new(EditFileTool::new(workspace.clone(), restrict)), IntentCategory::System);
    tools.register(Box::new(ListDirTool::new(workspace.clone(), restrict)), IntentCategory::System);
    tools.register(Box::new(ExecTool::new(
        workspace.clone(),
        restrict,
        config.tools.exec.timeout_seconds,
    )), IntentCategory::System);
    tools.register(Box::new(WebFetchTool::new(client.clone())), IntentCategory::Research);

    if !config.tools.web_search.api_key.is_empty() {
        let ws_key = crabbybot_core::vault::decrypt(&config.tools.web_search.api_key).unwrap_or_else(|e| {
            tracing::warn!("Failed to decrypt WebSearch API key: {}", e);
            config.tools.web_search.api_key.clone()
        });
        tools.register(Box::new(WebSearchTool::new(
            client.clone(),
            &ws_key,
            config.tools.web_search.max_results,
        )), IntentCategory::Research);
    }

    // Schedule tools (LLM-powered cron via natural language)
    if let Some(ref cron_arc) = cron {
        tools.register(Box::new(ScheduleTaskTool::new(
            Arc::clone(cron_arc),
            default_channel.to_string(),
            default_chat_id.to_string(),
        )), IntentCategory::System);
        tools.register(Box::new(ListSchedulesTool::new(Arc::clone(cron_arc))), IntentCategory::System);
        tools.register(Box::new(CancelScheduleTool::new(Arc::clone(cron_arc))), IntentCategory::System);
    }

    // Solana tools (crypto-native on-chain data)
    tools.register(Box::new(SolanaBalanceTool::new(
        client.clone(),
        &config.tools.solana_rpc_url,
    )), IntentCategory::CryptoTokens);
    tools.register(Box::new(SolanaTransactionsTool::new(
        client.clone(),
        &config.tools.solana_rpc_url,
    )), IntentCategory::CryptoTokens);
    tools.register(Box::new(SolanaTokenBalancesTool::new(
        client.clone(),
        &config.tools.solana_rpc_url,
    )), IntentCategory::CryptoTokens);

    // Polymarket read-only tools (markets, events, prices, data)
    let mut pm = config.tools.polymarket.clone();
    if let Some(ref pk) = pm.private_key {
        pm.private_key = Some(crabbybot_core::vault::decrypt(pk).unwrap_or_else(|e| {
            tracing::warn!("Failed to decrypt Polymarket private key: {}", e);
            pk.clone()
        }));
    }
    tools.register(Box::new(PolymarketTrendingTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketSearchTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketMarketTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketEventsTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketEventDetailTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketPriceTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketPriceHistoryTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketOrderbookTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketLastTradeTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketClobMarketTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketTickSizeTool::new(pm.clone())), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketPositionsTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketLeaderboardTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketClosedPositionsTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketTradesTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketActivityTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketHoldersTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketOpenInterestTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketVolumeTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketBuilderLeaderboardTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketBridgeTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketStatusTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketStreamTool::new()), IntentCategory::PolymarketRead);

    // Polymarket Gamma browsing (tags, series, comments, profiles, sports)
    tools.register(Box::new(PolymarketTagsTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketSeriesTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketCommentsTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketProfileTool::new()), IntentCategory::PolymarketRead);
    tools.register(Box::new(PolymarketSportsTool::new()), IntentCategory::PolymarketRead);

    // Polymarket authenticated trading tools (need POLYMARKET_PRIVATE_KEY)
    let pm = pm.clone();
    tools.register(Box::new(PolymarketCreateOrderTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketMarketOrderTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketMyOrdersTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketCancelOrderTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketBalanceTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketWalletTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketWalletCreateTool::new()), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketWalletImportTool::new()), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketRewardsTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketNotificationsTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketApiKeysTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketAccountStatusTool::new(pm.clone())), IntentCategory::PolymarketTrade);

    // Polymarket on-chain tools (need wallet + MATIC)
    tools.register(Box::new(PolymarketCtfSplitTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketCtfMergeTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketCtfRedeemTool::new(pm.clone())), IntentCategory::PolymarketTrade);
    tools.register(Box::new(PolymarketApproveTool::new(pm)), IntentCategory::PolymarketTrade);

    // Token Analysis
    tools.register(Box::new(RugCheckTool::new(client.clone())), IntentCategory::CryptoTokens);
    tools.register(Box::new(SentimentTool::new(client.clone())), IntentCategory::CryptoTokens);
    tools.register(Box::new(AlphaSummaryTool::new(client.clone())), IntentCategory::CryptoTokens);

    // Betting control tool (if betting state is provided)
    if let Some(ref bs) = betting_state {
        tools.register(Box::new(BettingControlTool::new(Arc::clone(bs))), IntentCategory::PolymarketTrade);
    }

    let agent_config = AgentConfig {
        model: model_override.map(|s| s.to_string()),
        max_tokens: config.agents.defaults.max_tokens,
        temperature: config.agents.defaults.temperature,
        max_iterations: config.agents.defaults.max_tool_iterations,
        workspace: workspace.clone(),
        max_context_tokens: 4_000,
    };

    // Prediction engine tools (share LLM provider via Arc<Mutex<...>>)
    let prediction_state = Arc::new(PredictionState {
        provider: Arc::clone(&provider),
        workspace: workspace.clone(),
    });
    tools.register(Box::new(PredictTool { state: Arc::clone(&prediction_state) }), IntentCategory::Prediction);
    tools.register(Box::new(SimulateTool { state: Arc::clone(&prediction_state) }), IntentCategory::Prediction);
    tools.register(Box::new(GraphQueryTool { workspace: workspace.clone() }), IntentCategory::Prediction);

    let tools = Arc::new(tools);
    let agent = AgentLoop::new(provider, Arc::clone(&tools), agent_config);
    Ok((agent, workspace, tools))
}

// ── Bot Command ─────────────────────────────────────────────────────

async fn cmd_bot() -> Result<()> {
    // 0. Ensure singleton execution via lock file to avoid Telegram session conflicts.
    let config_dir = Config::config_dir();
    let lock_path = config_dir.join("bot.lock");

    // Ensure config directory exists
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    // Try to create the lock file. Check if the PID inside is actually running.
    if lock_path.exists() {
        if let Ok(pid_str) = std::fs::read_to_string(&lock_path) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                use sysinfo::{Pid, System};
                let mut sys = System::new();
                sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                if sys.process(Pid::from_u32(pid)).is_some() {
                    anyhow::bail!(
                        "\n  \x1b[31m❌ Another instance of CrabbyBot is already running (PID {})!\x1b[0m\n\
                         \n     If you are sure it is not running, stop it or delete this file:\n\
                         \n     {}\n",
                        pid,
                        lock_path.display()
                    );
                }
            }
        }
        // If we reach here, the process is dead or invalid, so delete the stale lock.
        let _ = std::fs::remove_file(&lock_path);
    }
    std::fs::write(&lock_path, std::process::id().to_string())?;

    // Create a RAII guard to delete the lock file on exit.
    struct LockGuard(std::path::PathBuf);
    impl Drop for LockGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = LockGuard(lock_path.clone());

    // ── Restart loop ─────────────────────────────────────────────────
    loop {
        tracing::info!("Starting cmd_bot_once iteration...");
        let cancel = CancellationToken::new();
        match cmd_bot_once(cancel).await {
            Ok(_) => tracing::info!("cmd_bot_once finished successfully."),
            Err(e) => {
                tracing::error!("cmd_bot_once failed with error: {:?}", e);
                return Err(e);
            }
        }

        if !crabbybot_core::take_restart_request() {
            tracing::info!("No restart requested. Exiting loop.");
            break;
        }
        tracing::info!("Restart requested! Sleeping briefly before re-initializing...");
        println!("  🔄 Restarting CrabbyBot...");
        // Small delay to let sockets and resources clean up
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        tracing::info!("Wake up from sleep, starting new loop iteration.");
    }

    Ok(())
}

async fn cmd_bot_once(cancel: CancellationToken) -> Result<()> {
    let config = Config::load()?;
    validate_config(&config)?;

    let workspace = config.workspace_path();

    // Shared CronService for both the LLM tools and the cron ticker.
    let cron = Arc::new(tokio::sync::Mutex::new(CronService::new(&workspace)));

    // Derive default chat_id for cron jobs from the first allowed Telegram user.
    // In Telegram private chats, chat_id == user_id.
    let default_chat_id = config
        .channels
        .telegram
        .as_ref()
        .and_then(|t| t.allow_from.first())
        .cloned()
        .unwrap_or_default();

    let (bus, receivers) = crabbybot_core::bus::MessageBus::new(100);
    let bus_arc = Arc::new(bus);

    // 1.5 Initialize betting engine state
    let betting_state = Arc::new(tokio::sync::Mutex::new(
        BettingState::new(config.tools.betting.clone()),
    ));

    let (agent, workspace, tools_arc) = setup_agent(
        &config,
        None,
        Some(Arc::clone(&cron)),
        Arc::clone(&bus_arc),
        "telegram",
        &default_chat_id,
        Some(Arc::clone(&betting_state)),
    )?;

    let inbound_rx = receivers.inbound_rx;

    let mut services = tokio::task::JoinSet::new();

    println!("  🦀 CrabbyBot bot mode starting...");
    println!(
        "  Active channels: Telegram: {}, Discord: {}",
        config.channels.telegram.as_ref().is_some_and(|c| c.enabled),
        config.channels.discord.as_ref().is_some_and(|c| c.enabled)
    );
    {
        let cron_locked = cron.lock().await;
        println!("  Cron: {}", cron_locked.status());
    }
    println!("  Press Ctrl+C for graceful shutdown.");
    println!("  Betting: {}", if config.tools.betting.enabled { "🟢 ENABLED" } else { "🔴 DISABLED (use betting_control to start)" });
    println!("  ─────────────────────────────────────");

    // 1. Start transports FIRST so they register their outbound subscribers
    //    before the dispatch loop begins processing messages.

    #[cfg(feature = "telegram")]
    {
        if let Some(ref tel_config) = config.channels.telegram {
            if tel_config.enabled && !tel_config.token.is_empty() {
                let bus_for_tel = Arc::clone(&bus_arc);
                let allow_from = tel_config.allow_from.clone();
                let transport =
                    TelegramTransport::new(tel_config.token.clone(), bus_for_tel, allow_from, cancel.clone());
                services.spawn(async move {
                    if let Err(e) = transport.run().await {
                        tracing::error!("Telegram transport failed: {}", e);
                    }
                });
            }
        }
    }

    #[cfg(feature = "discord")]
    {
        if let Some(ref disc_config) = config.channels.discord {
            if disc_config.enabled && !disc_config.token.is_empty() {
                let bus_for_disc = Arc::clone(&bus_arc);
                let allow_from = disc_config.allow_from.clone();
                let transport =
                    DiscordTransport::new(disc_config.token.clone(), bus_for_disc, allow_from);
                services.spawn(async move {
                    if let Err(e) = transport.run().await {
                        tracing::error!("Discord transport failed: {}", e);
                    }
                });
            }
        }
    }

    if services.is_empty() {
        println!("  ⚠️ No bot channels enabled. Please check your config.");
        return Ok(());
    }

    // 2. Outbound Dispatcher — uses the shared subscriber map, no bus lock needed
    let subs = bus_arc.subscribers();
    services.spawn(async move {
        crabbybot_core::bus::dispatch_outbound(subs, receivers.outbound_rx).await;
    });

    // 3. Agent Bridge Task — with CancellationToken for graceful shutdown
    let bus_for_bridge = Arc::clone(&bus_arc);
    let bridge = AgentBridge::new(
        bus_for_bridge,
        agent,
        cancel.clone(),
        Arc::clone(&cron),
        workspace.clone(),
    );
    services.spawn(async move {
        if let Err(e) = bridge.run(inbound_rx).await {
            tracing::error!("Agent bridge failed: {}", e);
        }
    });

    // 3.5 Betting Engine — spawns the autonomous scan/trade loop
    {
        let betting_tools = Arc::clone(&tools_arc);
        let betting_st = Arc::clone(&betting_state);
        services.spawn(async move {
            let _ = BettingService::spawn(betting_st, betting_tools).await;
        });
    }

    // 4. Cron Ticker — checks for due jobs every 30 seconds.
    {
        let cron_tick = Arc::clone(&cron);
        let bus_tick = Arc::clone(&bus_arc);
        let cancel_tick = cancel.clone();
        services.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = cancel_tick.cancelled() => break,
                    _ = interval.tick() => {
                        let due_jobs = {
                            let mut cron_locked = cron_tick.lock().await;
                            cron_locked.get_due_jobs()
                        };
                        for job in due_jobs {
                            tracing::info!(
                                job_id = %job.id,
                                job_name = %job.name,
                                "Cron job fired"
                            );
                            if let Err(e) = bus_tick.inbound_sender().send(
                                crabbybot_core::bus::events::InboundMessage {
                                    channel: job.channel.clone(),
                                    chat_id: job.chat_id.clone(),
                                    user_id: "cron".to_string(),
                                    content: job.message.clone(),
                                    media: Vec::new(),
                                    is_system: true,
                                },
                            ).await {
                                tracing::error!("Failed to send cron job to bus: {}", e);
                            }
                        }
                    }
                }
            }
            tracing::info!("Cron ticker stopped");
        });
    }

    // Wait for cancel token, Ctrl+C, or for any critical service to exit unexpectedly.
    tokio::select! {
        _ = cancel.cancelled() => {
            tracing::info!("Shutdown signal received via CancellationToken!");
            println!("\n  ⏳ Shutting down gracefully...");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown signal received via Ctrl-C!");
            println!("\n  ⏳ Shutting down gracefully...");
        }
        res = services.join_next() => {
            if let Some(Err(e)) = res {
                tracing::error!("Critical service task panicked: {}", e);
            } else {
                tracing::warn!("A core service exited unexpectedly. Shutting down.");
            }
        }
    }

    tracing::info!("Cancelling remaining tasks...");
    cancel.cancel();
    
    tracing::info!("Waiting for services to cleanly shutdown (max 2s)...");
    // Give services a moment to clean up before hard-exiting.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), services.shutdown()).await;

    tracing::info!("Shutdown complete!");
    println!("  ✅ Shutdown complete.");
    Ok(())
}

// ── Chat Command ────────────────────────────────────────────────────

async fn cmd_chat(session_key: &str, model_override: Option<&str>) -> Result<()> {
    let config = Config::load()?;
    validate_config(&config)?;

    let model = model_override
        .unwrap_or(&config.agents.defaults.model)
        .to_string();
    let (bus, _receivers) = crabbybot_core::bus::MessageBus::new(10);
    let (mut agent, workspace, _tools_arc) = setup_agent(
        &config,
        model_override,
        None,
        Arc::new(bus),
        "cli",
        "direct",
        None,
    )?;

    // Print header
    println!();
    println!("  🦀 CrabbyBot v{}", env!("CARGO_PKG_VERSION"));
    println!(
        "  Providers: {} | Model: {}",
        config
            .providers
            .find_all_active()
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", "),
        model
    );
    println!(
        "  Session: {} | Workspace: {}",
        session_key,
        workspace.display()
    );
    println!();
    println!("  Type your message, or /quit to exit.");
    println!("  ─────────────────────────────────────");
    println!();

    // Interactive loop
    let stdin = io::stdin();
    loop {
        print!("  \x1b[36m>\x1b[0m ");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Handle commands
        match input {
            "/quit" | "/exit" | "/q" => {
                println!("  Goodbye! 👋");
                break;
            }
            "/clear" => {
                let mut mgr = SessionManager::new(&workspace);
                let session = mgr.get_or_create(session_key);
                session.clear();
                println!("  Session cleared.");
                continue;
            }
            "/status" => {
                cmd_status()?;
                continue;
            }
            _ => {}
        }

        // Process message — pass None because CLI doesn't need a bus for typing events
        println!();
        match agent.process(input, session_key, None).await {
            Ok(response) => {
                println!("  \x1b[32m{}\x1b[0m\n", response.content);
            }
            Err(e) => {
                eprintln!("  \x1b[31mError: {}\x1b[0m\n", e);
            }
        }
    }

    Ok(())
}

// ── Onboard Command ─────────────────────────────────────────────────

fn cmd_onboard() -> Result<()> {
    let path = Config::write_default_template()?;
    println!();
    println!("  ✅ Configuration created at:");
    println!("     {}", path.display());
    println!();
    println!("  Next steps:");
    println!("  1. Edit the config file and add your API key");
    println!("  2. Run `CrabbyBot chat` to start chatting");
    println!();
    Ok(())
}

// ── Status Command ──────────────────────────────────────────────────

fn cmd_status() -> Result<()> {
    let config_path = Config::default_path();
    let config = Config::load()?;

    println!();
    println!("  🦀 CrabbyBot status");
    println!("  ─────────────────────────────────────");

    // Config file
    if config_path.exists() {
        println!("  Config:    {}", config_path.display());
    } else {
        println!("  Config:    ❌ Not found (run `CrabbyBot onboard`)");
        return Ok(());
    }

    // Provider
    match config.providers.find_active() {
        Some((name, _)) => println!("  Provider:  ✅ {} configured", name),
        None => println!("  Provider:  ❌ No provider configured"),
    }

    // Model
    println!("  Model:     {}", config.agents.defaults.model);

    // Workspace
    let ws = config.workspace_path();
    let ws_exists = ws.exists();
    println!(
        "  Workspace: {} {}",
        ws.display(),
        if ws_exists {
            "✅"
        } else {
            "⚠️  (will be created)"
        }
    );

    // Sessions
    let mgr = SessionManager::new(&ws);
    let sessions = mgr.list_sessions();
    println!("  Sessions:  {} saved", sessions.len());

    // Cron
    let cron = CronService::new(&ws);
    println!("  Cron:      {}", cron.status());

    println!();
    Ok(())
}

// ── Cron Commands ───────────────────────────────────────────────────

fn cmd_cron(action: CronCommands) -> Result<()> {
    let config = Config::load()?;
    let ws = config.workspace_path();
    let mut cron = CronService::new(&ws);

    match action {
        CronCommands::List => {
            let jobs = cron.list_jobs(true);
            if jobs.is_empty() {
                println!("  No scheduled jobs.");
            } else {
                println!();
                for job in jobs {
                    let status = if job.enabled { "✅" } else { "⏸️ " };
                    println!("  {} {} [{}]", status, job.name, job.id);
                    match &job.schedule {
                        Schedule::Cron { expression } => println!("     Cron: {}", expression),
                        Schedule::Interval { seconds } => {
                            println!("     Every {} seconds", seconds)
                        }
                    }
                    println!("     Message: {}", job.message);
                    if let Some(ref last) = job.last_run {
                        println!("     Last run: {}", last);
                    }
                    println!();
                }
            }
        }
        CronCommands::Add {
            name,
            schedule,
            message,
        } => {
            let sched = Schedule::Cron {
                expression: schedule,
            };
            let id = cron.add_job(&name, sched, &message, "cli", "direct")?;
            println!("  ✅ Job added: {} ({})", name, id);
        }
        CronCommands::Remove { id } => {
            if cron.remove_job(&id)? {
                println!("  ✅ Job removed: {}", id);
            } else {
                println!("  ❌ Job not found: {}", id);
            }
        }
    }

    Ok(())
}

// ── Session Commands ────────────────────────────────────────────────

fn cmd_sessions(action: Option<SessionCommands>) -> Result<()> {
    let config = Config::load()?;
    let ws = config.workspace_path();
    let mut mgr = SessionManager::new(&ws);

    match action {
        Some(SessionCommands::Delete { key }) => {
            if mgr.delete(&key) {
                println!("  ✅ Session deleted: {}", key);
            } else {
                println!("  ❌ Session not found: {}", key);
            }
        }
        Some(SessionCommands::List) | None => {
            let sessions = mgr.list_sessions();
            if sessions.is_empty() {
                println!("  No saved sessions.");
            } else {
                println!();
                for (key, updated) in sessions {
                    println!("  📝 {} (updated: {})", key, updated);
                }
                println!();
            }
        }
    }

    Ok(())
}

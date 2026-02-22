//! 🦀 crabbybot CLI — interactive chat, onboarding, and status commands.
//!
//! Usage:
//!   crabbybot chat          — Start an interactive chat session
//!   crabbybot onboard       — Create a default configuration
//!   crabbybot status        — Show current configuration and health
//!   crabbybot cron list      — List scheduled jobs
//!   crabbybot sessions       — List conversation sessions

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crabbybot_core::agent::{AgentConfig, AgentLoop};
use crabbybot_core::config::Config;
use crabbybot_core::cron::{CronService, Schedule};
use crabbybot_core::provider::openai::OpenAiProvider;
use crabbybot_core::provider::LlmProvider;
use crabbybot_core::session::SessionManager;
use crabbybot_core::tools::filesystem::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
use crabbybot_core::tools::polymarket::{PolymarketTrendingTool, PolymarketSearchTool, PolymarketMarketTool};
use crabbybot_core::tools::pumpfun::{PumpFunTokenTool, PumpFunSearchTool};
use crabbybot_core::tools::alpha_summary::AlphaSummaryTool;
use crabbybot_core::tools::pumpfun_buy::PumpFunBuyTool;
use crabbybot_core::tools::rugcheck::RugCheckTool;
use crabbybot_core::tools::sentiment::SentimentTool;
use crabbybot_core::tools::schedule::{CancelScheduleTool, ListSchedulesTool, ScheduleTaskTool};
use crabbybot_core::tools::shell::ExecTool;
use crabbybot_core::tools::solana::{SolanaBalanceTool, SolanaTokenBalancesTool, SolanaTransactionsTool};
use crabbybot_core::tools::web::{WebFetchTool, WebSearchTool};
use crabbybot_core::tools::ToolRegistry;
use crabbybot_core::gateway::AgentBridge;
#[cfg(feature = "telegram")]
use crabbybot_core::gateway::channels::telegram::TelegramTransport;
#[cfg(feature = "discord")]
use crabbybot_core::gateway::channels::discord::DiscordTransport;

#[derive(Parser)]
#[command(
    name = "crabbybot",
    version,
    about = "An ultra-lightweight personal AI assistant",
    long_about = "🦀 crabbybot — a blazing-fast AI assistant written in Rust.\n\nZero runtime dependencies. Single binary. Direct LLM API access."
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

// ── Shared Setup ────────────────────────────────────────────────────

/// Shared helper that loads config, validates it, and builds a fully
/// wired `AgentLoop` with providers and tools.
///
/// Returns `(agent, config, workspace_path)` so both `cmd_chat` and
/// `cmd_bot` can avoid duplicating this boilerplate.
fn validate_config(config: &Config) -> Result<()> {
    if let Err(errors) = config.validate() {
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
    default_channel: &str,
    default_chat_id: &str,
) -> Result<(AgentLoop, PathBuf)> {
    let model = model_override
        .unwrap_or(&config.agents.defaults.model)
        .to_string();

    // Resolve providers
    let active_providers = config.providers.find_all_active();
    if active_providers.is_empty() {
        anyhow::bail!(
            "No LLM provider configured with a real API key. \
             Run `crabbybot onboard` first, then edit config.json"
        );
    }

    let client = reqwest::Client::new();
    let mut inner_providers = Vec::new();
    for (name, entry) in active_providers {
        let p_model = entry.model.as_deref().unwrap_or(&model);
        let p = OpenAiProvider::new(
            name,
            &entry.api_key,
            entry.api_base.as_deref(),
            p_model,
            client.clone(),
        );
        inner_providers.push((name.to_string(), Box::new(p) as Box<dyn LlmProvider>));
    }

    let provider = crabbybot_core::provider::FallbackProvider::new(inner_providers);

    // Set up tools
    let workspace = config.workspace_path();
    let restrict = config.tools.restrict_to_workspace;
    let mut tools = ToolRegistry::new();

    tools.register(Box::new(ReadFileTool::new(workspace.clone(), restrict)));
    tools.register(Box::new(WriteFileTool::new(workspace.clone(), restrict)));
    tools.register(Box::new(EditFileTool::new(workspace.clone(), restrict)));
    tools.register(Box::new(ListDirTool::new(workspace.clone(), restrict)));
    tools.register(Box::new(ExecTool::new(
        workspace.clone(),
        restrict,
        config.tools.exec.timeout_seconds,
    )));
    tools.register(Box::new(WebFetchTool::new(client.clone())));

    if !config.tools.web_search.api_key.is_empty() {
        tools.register(Box::new(WebSearchTool::new(
            client.clone(),
            &config.tools.web_search.api_key,
            config.tools.web_search.max_results,
        )));
    }

    // Schedule tools (LLM-powered cron via natural language)
    if let Some(ref cron_arc) = cron {
        tools.register(Box::new(ScheduleTaskTool::new(
            Arc::clone(cron_arc),
            default_channel.to_string(),
            default_chat_id.to_string(),
        )));
        tools.register(Box::new(ListSchedulesTool::new(Arc::clone(cron_arc))));
        tools.register(Box::new(CancelScheduleTool::new(Arc::clone(cron_arc))));
    }

    // Solana tools (crypto-native on-chain data)
    tools.register(Box::new(SolanaBalanceTool::new(client.clone(), &config.tools.solana_rpc_url)));
    tools.register(Box::new(SolanaTransactionsTool::new(client.clone(), &config.tools.solana_rpc_url)));
    tools.register(Box::new(SolanaTokenBalancesTool::new(client.clone(), &config.tools.solana_rpc_url)));

    // Pump.fun tools 
    tools.register(Box::new(PumpFunTokenTool::new(client.clone())));
    tools.register(Box::new(PumpFunSearchTool::new(client.clone())));

    // Polymarket prediction-market tools
    tools.register(Box::new(PolymarketTrendingTool::new()));
    tools.register(Box::new(PolymarketSearchTool::new()));
    tools.register(Box::new(PolymarketMarketTool::new()));

    // Token Analysis
    tools.register(Box::new(RugCheckTool::new(client.clone())));
    tools.register(Box::new(SentimentTool::new(client.clone())));
    tools.register(Box::new(AlphaSummaryTool::new(client.clone())));
    tools.register(Box::new(PumpFunBuyTool::new(
        client.clone(),
        &config.tools.solana_rpc_url,
        config.tools.solana_private_key.clone(),
    )));

    let agent_config = AgentConfig {
        model: model_override.map(|s| s.to_string()),
        max_tokens: config.agents.defaults.max_tokens,
        temperature: config.agents.defaults.temperature,
        max_iterations: config.agents.defaults.max_tool_iterations,
        workspace: workspace.clone(),
        max_context_tokens: 30_000,
    };

    let agent = AgentLoop::new(Box::new(provider), tools, agent_config);
    Ok((agent, workspace))
}

// ── Bot Command ─────────────────────────────────────────────────────

async fn cmd_bot() -> Result<()> {
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

    let (agent, _workspace) = setup_agent(&config, None, Some(Arc::clone(&cron)), "telegram", &default_chat_id)?;

    let (bus, receivers) = crabbybot_core::bus::MessageBus::new(100);
    let bus_arc = Arc::new(bus);

    let mut tasks = Vec::new();
    let inbound_rx = receivers.inbound_rx;

    // 1. Start transports FIRST so they register their outbound subscribers
    //    before the dispatch loop begins processing messages.

    #[cfg(feature = "telegram")]
    {
        if let Some(ref tel_config) = config.channels.telegram {
            if tel_config.enabled && !tel_config.token.is_empty() {
                let bus_for_tel = Arc::clone(&bus_arc);
                let allow_from = tel_config.allow_from.clone();
                let transport = TelegramTransport::new(
                    tel_config.token.clone(),
                    bus_for_tel,
                    allow_from,
                );
                tasks.push(tokio::spawn(async move {
                    if let Err(e) = transport.run().await {
                        tracing::error!("Telegram transport failed: {}", e);
                    }
                }));
            }
        }
    }

    #[cfg(feature = "discord")]
    {
        if let Some(ref disc_config) = config.channels.discord {
            if disc_config.enabled && !disc_config.token.is_empty() {
                let bus_for_disc = Arc::clone(&bus_arc);
                let allow_from = disc_config.allow_from.clone();
                let transport = DiscordTransport::new(
                    disc_config.token.clone(),
                    bus_for_disc,
                    allow_from,
                );
                tasks.push(tokio::spawn(async move {
                    if let Err(e) = transport.run().await {
                        tracing::error!("Discord transport failed: {}", e);
                    }
                }));
            }
        }
    }

    if tasks.is_empty() {
        println!("  ⚠️ No bot channels enabled. Please check your config.");
        return Ok(());
    }

    // 2. Outbound Dispatcher — uses the shared subscriber map, no bus lock needed
    let subs = bus_arc.subscribers();
    tasks.push(tokio::spawn(async move {
        crabbybot_core::bus::dispatch_outbound(subs, receivers.outbound_rx).await;
    }));

    // 3. Agent Bridge Task — with CancellationToken for graceful shutdown
    let cancel = CancellationToken::new();
    let bus_for_bridge = Arc::clone(&bus_arc);
    let bridge = AgentBridge::new(
        bus_for_bridge,
        agent,
        cancel.clone(),
        Arc::clone(&cron),
        workspace.clone(),
    );
    tasks.push(tokio::spawn(async move {
        if let Err(e) = bridge.run(inbound_rx).await {
            tracing::error!("Agent bridge failed: {}", e);
        }
    }));

    // 4. Cron Ticker — checks for due jobs every 30 seconds.
    {
        let cron_tick = Arc::clone(&cron);
        let bus_tick = Arc::clone(&bus_arc);
        let cancel_tick = cancel.clone();
        tasks.push(tokio::spawn(async move {
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
        }));
    }

    println!("  🦀 crabbybot bot mode starting...");
    println!(
        "  Active channels: Telegram: {}, Discord: {}",
        config.channels.telegram.as_ref().map_or(false, |c| c.enabled),
        config.channels.discord.as_ref().map_or(false, |c| c.enabled)
    );
    {
        let cron_locked = cron.lock().await;
        println!("  Cron: {}", cron_locked.status());
    }
    println!("  Press Ctrl+C for graceful shutdown.");
    println!("  ─────────────────────────────────────");

    // Wait for Ctrl+C, then cancel the bridge gracefully.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\n  ⏳ Shutting down gracefully...");
            cancel.cancel();
        }
        _ = async { futures::future::join_all(tasks).await } => {
            // All tasks finished on their own.
        }
    }

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
    let (mut agent, workspace) = setup_agent(&config, model_override, None, "cli", "direct")?;

    // Print header
    println!();
    println!("  🦀 crabbybot v{}", env!("CARGO_PKG_VERSION"));
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
    println!("  Session: {} | Workspace: {}", session_key, workspace.display());
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
        print!("\n");
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
    println!("  2. Run `crabbybot chat` to start chatting");
    println!();
    Ok(())
}

// ── Status Command ──────────────────────────────────────────────────

fn cmd_status() -> Result<()> {
    let config_path = Config::default_path();
    let config = Config::load()?;

    println!();
    println!("  🦀 crabbybot status");
    println!("  ─────────────────────────────────────");

    // Config file
    if config_path.exists() {
        println!("  Config:    {}", config_path.display());
    } else {
        println!("  Config:    ❌ Not found (run `crabbybot onboard`)");
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
        if ws_exists { "✅" } else { "⚠️  (will be created)" }
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

//! ferrobot CLI — interactive chat, onboarding, and status commands.
//!
//! Usage:
//!   ferrobot chat          — Start an interactive chat session
//!   ferrobot onboard       — Create a default configuration
//!   ferrobot status        — Show current configuration and health
//!   ferrobot cron list     — List scheduled jobs
//!   ferrobot sessions      — List conversation sessions

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{self, Write};


use ferrobot_core::agent::{AgentConfig, AgentLoop};
use ferrobot_core::config::Config;
use ferrobot_core::cron::{CronService, Schedule};
use ferrobot_core::provider::openai::OpenAiProvider;
use ferrobot_core::session::SessionManager;
use ferrobot_core::tools::filesystem::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
use ferrobot_core::tools::shell::ExecTool;
use ferrobot_core::tools::web::{WebFetchTool, WebSearchTool};
use ferrobot_core::tools::ToolRegistry;

#[derive(Parser)]
#[command(
    name = "ferrobot",
    version,
    about = "An ultra-lightweight personal AI assistant",
    long_about = "ferrobot — a blazing-fast AI assistant written in Rust.\n\nZero runtime dependencies. Single binary. Direct LLM API access."
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
        Some(Commands::Onboard) => cmd_onboard()?,
        Some(Commands::Status) => cmd_status()?,
        Some(Commands::Cron { action }) => cmd_cron(action)?,
        Some(Commands::Sessions { action }) => cmd_sessions(action)?,
        None => cmd_chat("default", None).await?,
    }

    Ok(())
}

// ── Chat Command ────────────────────────────────────────────────────

async fn cmd_chat(session_key: &str, model_override: Option<&str>) -> Result<()> {
    let config = Config::load()?;

    // Resolve provider
    let (provider_name, entry) = config
        .providers
        .find_active()
        .ok_or_else(|| anyhow::anyhow!(
            "No LLM provider configured. Run `ferrobot onboard` first, then edit ~/.ferrobot/config.json"
        ))?;

    let model = model_override
        .unwrap_or(&config.agents.defaults.model)
        .to_string();

    let provider = OpenAiProvider::new(
        provider_name,
        &entry.api_key,
        entry.api_base.as_deref(),
        &model,
    );

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
    tools.register(Box::new(WebFetchTool::new()));

    if !config.tools.web_search.api_key.is_empty() {
        tools.register(Box::new(WebSearchTool::new(
            &config.tools.web_search.api_key,
            config.tools.web_search.max_results,
        )));
    }

    let agent_config = AgentConfig {
        model: model.clone(),
        max_tokens: config.agents.defaults.max_tokens,
        temperature: config.agents.defaults.temperature,
        max_iterations: config.agents.defaults.max_tool_iterations,
        workspace: workspace.clone(),
    };

    let mut agent = AgentLoop::new(Box::new(provider), tools, agent_config);

    // Print header
    println!();
    println!("  🤖 ferrobot v{}", env!("CARGO_PKG_VERSION"));
    println!("  Provider: {} | Model: {}", provider_name, model);
    println!("  Session: {} | Workspace: {}", session_key, workspace.display());
    println!("  {} tools loaded", 6 + if config.tools.web_search.api_key.is_empty() { 0 } else { 1 });
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
                let ws = config.workspace_path();
                let mut mgr = SessionManager::new(&ws);
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

        // Process message
        print!("\n");
        match agent.process(input, session_key).await {
            Ok(response) => {
                println!("  \x1b[32m{}\x1b[0m\n", response);
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
    println!("  2. Run `ferrobot chat` to start chatting");
    println!();
    Ok(())
}

// ── Status Command ──────────────────────────────────────────────────

fn cmd_status() -> Result<()> {
    let config_path = Config::default_path();
    let config = Config::load()?;

    println!();
    println!("  🤖 ferrobot status");
    println!("  ─────────────────────────────────────");

    // Config file
    if config_path.exists() {
        println!("  Config:    {}", config_path.display());
    } else {
        println!("  Config:    ❌ Not found (run `ferrobot onboard`)");
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
            let id = cron.add_job(&name, sched, &message)?;
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

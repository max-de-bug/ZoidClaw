# 🤖 ferrobot

**An ultra-lightweight personal AI assistant, written in Rust.**

> Inspired by OpenClaw — blazing-fast, zero runtime dependencies, single binary.

---

## Why ferrobot?

Ferrobot is designed for developers who want a local, scriptable AI assistant without the bloat of Python environments or heavy runtime dependencies.

- **Single ~5MB binary** vs heavy multi-hundred MB environments.
- **~5ms Cold start** — instant responsiveness.
- **Direct HTTP** — no intermediate wrappers like LiteLLM.
- **True multi-threaded** via Tokio for high-performance tool execution.
- **Compile-time safety** for configuration and tool parameters.

## Features

- **Direct LLM API access** — No LiteLLM middleman. Works with OpenAI, Anthropic, DeepSeek, Groq, Gemini, OpenRouter, or any vLLM endpoint.
- **Tool calling** — Read/write/edit files, execute shell commands, search the web, fetch pages.
- **Persistent memory** — Daily notes and long-term memory in plain Markdown.
- **Session management** — JSONL-persisted conversation history.
- **Skills system** — Learn new capabilities from Markdown-based skill files.
- **Cron scheduler** — Schedule recurring tasks with cron expressions.
- **Extensible** — Add tools and channels via Rust traits.

## Quick Start

```bash
# Build from source
cargo build --release

# First-time setup (creates ~/.ferrobot/config.json)
./target/release/ferrobot onboard

# Edit config with your API key
# Then start chatting:
./target/release/ferrobot chat
```

## Commands

```bash
ferrobot              # Start interactive chat (default session)
ferrobot chat         # Start interactive chat
ferrobot chat -s work # Use named session
ferrobot chat -m gpt-4o  # Override model
ferrobot onboard      # Create default config
ferrobot status       # Show config & health
ferrobot cron list    # List scheduled jobs
ferrobot sessions     # Manage sessions
```

## Configuration

Located at `~/.ferrobot/config.json`:

```json
{
  "providers": {
    "openrouter": {
      "apiKey": "sk-or-v1-YOUR_KEY_HERE"
    }
  },
  "agents": {
    "defaults": {
      "model": "anthropic/claude-sonnet-4-5",
      "maxTokens": 8192,
      "temperature": 0.7
    }
  },
  "tools": {
    "webSearch": {
      "apiKey": "YOUR_BRAVE_API_KEY"
    }
  }
}
```

## Architecture

```
ferrobot/
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── ferrobot-core/         # Library crate
│   │   └── src/
│   │       ├── lib.rs         # Module declarations
│   │       ├── config/        # Typed JSON config
│   │       ├── provider/      # LLM provider trait + OpenAI HTTP client
│   │       ├── bus/           # Async message bus (tokio mpsc)
│   │       ├── tools/         # Tool trait + registry + built-in tools
│   │       ├── agent/         # Agent loop, memory, skills, context
│   │       ├── session/       # JSONL session persistence
│   │       └── cron/          # Cron scheduler
│   └── ferrobot-cli/          # Binary crate
│       └── src/
│           └── main.rs        # CLI with clap
└── README.md
```

## License

MIT

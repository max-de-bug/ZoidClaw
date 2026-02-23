<div>
  <table>
    <tr>
      <td valign="top" width="220">
        <img src="assets/images/crabbybot_logo.png" alt="Crabbybot Logo" width="200">
      </td>
      <td valign="top">
        <h1>🦀 Crabbybot</h1>
        <a href="https://git.io/typing-svg">
          <img src="https://readme-typing-svg.demolab.com?font=Fira+Code&weight=600&size=22&duration=3000&pause=1000&color=F74C00&vCenter=true&width=550&lines=The+high-performance+Rust+AI+bridge.;Asynchronous.+Concurrent.+Blazing+fast.;CLI%2C+Telegram+%26+Discord+integration.;Zero+runtime+dependencies.+Pure+Rust." alt="Typing SVG" />
        </a>
        <br><br>
        <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-stable-brightgreen.svg" alt="Rust"></a>
        <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
        <a href="https://github.com/max-de-bug/crabbybot"><img src="https://img.shields.io/badge/platform-windows%20%7C%20linux%20%7C%20macos-lightgrey" alt="Platform"></a>
      </td>
    </tr>
  </table>
</div>

**Meet Crabbybot:** an ultra-lightweight, OpenClaw-style AI assistant that deploys in seconds. Written in pure Rust with a tiny ~5MB footprint, it delivers blazing-fast local execution, seamless Telegram integration, and a commanding edge with native Pump.fun & Solana tools.

## 🚀 Key Features

- **⚡ Crypto Native**: First-class support for **Solana** and **Pump.fun**, including real-time alerts, rug detection, and alpha scoring.
- **💬 Multi-Channel**: Native bridges for **Telegram**, **Discord**, and a powerful **CLI**.
- **🎯 Shortcut Commands**: High-velocity slash commands (`/portfolio`, `/alpha`, `/buy`) for instant on-chain interaction.
- **⏰ Proactive Autonomy**: Integrated cron engine for scheduling recurring AI research and monitoring tasks.
- **🛠️ Extensible Tool-Use**: Native capability to execute shell commands, manage files, and fetch live web data.
- **🔐 Session Persistence**: Persistent conversation threads stored locally and securely.
- **🦀 Pure Rust Core**: Zero runtime dependencies and sub-millisecond local routing.

## 🏗️ Architecture

Decoupled, event-driven, and concurrent.

```mermaid
graph TD
    User([User])
    subgraph "Transports"
        CLI[CLI]
        TG[Telegram]
        DC[Discord]
    end
    Bus{Message Bus}
    Bridge[Agent Bridge]
    Loop[Agent Loop]
    LLM(LLM Grid)
    Tools[Tool Registry]

    User <--> CLI
    User <--> TG
    User <--> DC
    
    CLI <--> Bus
    TG <--> Bus
    DC <--> Bus
    
    Bus <--> Bridge
    Bridge <--> Loop
    Loop <--> LLM
    Loop <--> Tools
```

## 🛠️ Deployment

### Prerequisites
- [Rust Toolchain](https://rustup.rs/)

### Build
1. **Clone & Compile**:
    ```bash
    git clone https://github.com/max-de-bug/crabbybot.git
    cd crabbybot
    cargo build --release
    ```
2. **Onboard**:
    ```bash
    ./target/release/crabbybot onboard
    ```

## ⚙️ Configuration

Crabbybot is configured via `~/.crabbybot/config.json`. 

```json
{
  "providers": {
    "openrouter": {
      "apiKey": "YOUR_OPENROUTER_KEY"
    }
  },
  "agents": {
    "defaults": {
      "model": "anthropic/claude-3-5-sonnet",
      "workspace": "~/.crabbybot/workspace"
    }
  },
  "channels": {
    "telegram": {
      "enabled": true,
      "token": "YOUR_TELEGRAM_TOKEN"
    },
    "discord": {
      "enabled": false,
      "token": "YOUR_DISCORD_TOKEN"
    }
  }
}
```

## 🤖 Usage

### Interactive Chat (CLI)
Start a standard interactive session:
```bash
crabbybot chat
```

### Bot Mode (Telegram/Discord)
Run Crabbybot in the background to serve external channels:
```bash
crabbybot bot
```

### Scheduling Jobs
Add a cron job to keep you updated:
```bash
crabbybot cron add --name "Morning Brief" --schedule "0 8 * * *" --message "Summarize the latest AI news."
```

## 📡 Channel Setup

### Telegram
1. Message [@BotFather](https://t.me/botfather) to create a bot and get a token.
2. Enable `telegram` in your `config.json`.
3. Run `crabbybot bot`.

### Discord
1. Create an app on the [Discord Developer Portal](https://discord.com/developers/applications).
2. Add a Bot, enable `Message Content Intent`.
3. Enable `discord` in your `config.json`.
4. Run `crabbybot bot`.

## 🛡️ License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---
*Built with 🦀 for the Solana Ecosystem.*

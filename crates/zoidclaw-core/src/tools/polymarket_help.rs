//! Polymarket CLI help text — shown when a user sends `/polymarket --help`.
//!
//! Derived from the official Polymarket CLI README.
//! Kept in a dedicated module so the transport layer stays clean.

/// Full command reference for the Polymarket CLI.
///
/// Formatted for Telegram (plain text with emojis, no HTML/Markdown
/// parse-mode issues).
pub const POLYMARKET_HELP: &str = "\
📖 Polymarket CLI — Command Reference

━━━ 📊 Markets ━━━
/polymarket markets list --limit 10
/polymarket markets list --active true --order volume_num
/polymarket markets get <ID_or_SLUG>
/polymarket markets search \"<query>\" --limit 5
/polymarket markets tags <MARKET_ID>

━━━ 📅 Events ━━━
/polymarket events list --limit 10
/polymarket events list --tag politics --active true
/polymarket events get <EVENT_ID>
/polymarket events tags <EVENT_ID>

━━━ 🏷 Tags / Series / Comments / Profiles / Sports ━━━
/polymarket tags list
/polymarket tags get <TAG>
/polymarket tags related <TAG>
/polymarket series list --limit 10
/polymarket series get <SERIES_ID>
/polymarket comments list --entity-type event --entity-id <ID>
/polymarket profiles get <ADDRESS>
/polymarket sports list
/polymarket sports market-types
/polymarket sports teams --league NFL --limit 32

━━━ 📈 Order Book & Prices (read-only) ━━━
/polymarket clob ok
/polymarket clob price <TOKEN_ID> --side buy
/polymarket clob midpoint <TOKEN_ID>
/polymarket clob spread <TOKEN_ID>
/polymarket clob book <TOKEN_ID>
/polymarket clob last-trade <TOKEN_ID>
/polymarket clob market <CONDITION_ID>
/polymarket clob markets
/polymarket clob price-history <TOKEN_ID> --interval 1d --fidelity 30
/polymarket clob tick-size <TOKEN_ID>

━━━ 💰 Trading (wallet required) ━━━
/polymarket clob create-order --token <ID> --side buy --price 0.50 --size 10
/polymarket clob market-order --token <ID> --side buy --amount 5
/polymarket clob cancel <ORDER_ID>
/polymarket clob cancel-all
/polymarket clob orders
/polymarket clob order <ORDER_ID>
/polymarket clob trades
/polymarket clob balance --asset-type collateral

━━━ 🏆 Rewards & API Keys ━━━
/polymarket clob rewards --date 2024-06-15
/polymarket clob current-rewards
/polymarket clob api-keys
/polymarket clob create-api-key
/polymarket clob account-status
/polymarket clob notifications

━━━ 📊 On-Chain Data ━━━
/polymarket data positions <WALLET>
/polymarket data closed-positions <WALLET>
/polymarket data trades <WALLET> --limit 50
/polymarket data activity <WALLET>
/polymarket data holders <CONDITION_ID>
/polymarket data open-interest <CONDITION_ID>
/polymarket data volume <EVENT_ID>
/polymarket data leaderboard --period month --order-by pnl --limit 10

━━━ ✅ Approvals ━━━
/polymarket approve check
/polymarket approve set

━━━ 🔀 CTF Operations ━━━
/polymarket ctf split --condition <ID> --amount 10
/polymarket ctf merge --condition <ID> --amount 10
/polymarket ctf redeem --condition <ID>

━━━ 🌉 Bridge ━━━
/polymarket bridge deposit <WALLET>
/polymarket bridge supported-assets
/polymarket bridge status <DEPOSIT_ADDRESS>

━━━ 👛 Wallet ━━━
/polymarket wallet create
/polymarket wallet import <KEY>
/polymarket wallet address
/polymarket wallet show
/polymarket wallet reset

━━━ 🔧 System ━━━
/polymarket status
";

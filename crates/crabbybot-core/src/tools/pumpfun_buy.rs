//! Solomon token buying interface for Pump.fun.
//!
//! Uses solana-tracker.io for swap transaction generation and solana-sdk for signing.

use super::Tool;
use super::rugcheck::RugCheckTool;
use async_trait::async_trait;
use base64::prelude::*;
use serde::Deserialize;
use serde_json::{json, Value};
use solana_transaction::versioned::VersionedTransaction;
use solana_pubkey::Pubkey;
use std::collections::HashMap;
use tracing::debug;

const SOLANA_TRACKER_API: &str = "https://api.solanatracker.io";

#[derive(Debug, Deserialize)]
struct SwapResponse {
    #[serde(rename = "txn")]
    transaction_base64: String,
}

pub struct PumpFunBuyTool {
    client: reqwest::Client,
    private_key: Option<String>,
    rpc_url: String,
    rugcheck: RugCheckTool,
}

impl PumpFunBuyTool {
    pub fn new(client: reqwest::Client, rpc_url: &str, private_key: Option<String>) -> Self {
        Self {
            client: client.clone(),
            private_key,
            rpc_url: rpc_url.to_string(),
            rugcheck: RugCheckTool::new(client),
        }
    }

    fn get_signer_key(&self) -> Result<ed25519_dalek::SigningKey, String> {
        let key_str = self.private_key.as_ref()
            .ok_or("❌ No Solana private key configured. Use the `onboard` command or edit `config.json`.")?;
        
        let bytes = bs58::decode(key_str).into_vec()
            .map_err(|e| format!("❌ Invalid private key format (base58 error): {}", e))?;
        
        if bytes.len() < 32 {
            return Err("❌ Invalid private key: too short".into());
        }

        Ok(ed25519_dalek::SigningKey::from_bytes(&bytes[0..32].try_into().unwrap()))
    }
}

#[async_trait]
impl Tool for PumpFunBuyTool {
    fn name(&self) -> &str {
        "pumpfun_buy"
    }

    fn description(&self) -> &str {
        "Buy a Solana token (specifically on Pump.fun) using SOL. \
         This is a 2-step process: \
         1. First call with `confirm=false` to get a quote and safety report. \
         2. Second call with `confirm=true` to execute the transaction after user approval. \
         Parameters: `mint` (CA), `amount_sol` (SOL amount), `confirm` (bool)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mint": {
                    "type": "string",
                    "description": "The token contract address (mint)"
                },
                "amount_sol": {
                    "type": "number",
                    "description": "Amount of SOL to spend (e.g. 0.1)"
                },
                "confirm": {
                    "type": "boolean",
                    "description": "Set to true ONLY after the user has seen the quote and manually said 'confirm' or similar."
                }
            },
            "required": ["mint", "amount_sol", "confirm"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let mint = match args.get("mint").and_then(|v| v.as_str()) {
            Some(m) => m,
            None => return "❌ Error: 'mint' is required".into(),
        };
        let amount_sol = match args.get("amount_sol").and_then(|v| v.as_f64()) {
            Some(a) => a,
            None => return "❌ Error: 'amount_sol' is required".into(),
        };
        let confirm = args.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);

        // Security check: Rugcheck
        let rug_report = match self.rugcheck.fetch_report(mint).await {
            Ok(r) => r,
            Err(e) => return format!("⚠️ Could not perform safety check: {}\nAborting for safety.", e),
        };

        if !confirm {
            let safety_icon = if rug_report.score < 2000 { "🟢" } else if rug_report.score < 5000 { "🟡" } else { "🔴" };
            
            let mut summary = format!(
                "📠 **Buy Quote: {}** (${})\n\
                 Spend: **{:.4} SOL**\n\n\
                 🛡️ **Safety Check**: {} Score **{}**\n\
                 {}\n\n\
                 ⚠️ **WARNING**: Memecoin trading is extremely high risk. \n\
                 To proceed, please reply with **'Confirm Buy'**.",
                rug_report.file_meta.name, rug_report.file_meta.symbol,
                amount_sol,
                safety_icon, rug_report.score,
                if rug_report.score >= 5000 { "🛑 **HIGH RISK DETECTED**" } else { "✅ Ready for execution" }
            );

            // Hidden UI marker for the agent/transport to detect and add buttons
            summary.push_str(&format!("\n\n[UI_CONFIRM_BUY: {} | {}]", mint, amount_sol));
            
            return summary;
        }

        // Execution phase
        let signing_key = match self.get_signer_key() {
            Ok(k) => k,
            Err(e) => return e,
        };
        let payer_pubkey = Pubkey::from(signing_key.verifying_key().to_bytes());

        debug!(mint, amount_sol, "Requesting swap transaction from SolanaTracker");
        
        let url = format!("{}/swap", SOLANA_TRACKER_API);
        let params = json!({
            "from": "So11111111111111111111111111111111111111112", // SOL
            "to": mint,
            "fromAmount": amount_sol,
            "slippage": 15, // Standard for pump.fun
            "payer": payer_pubkey.to_string(),
            "forceLegacy": false
        });

        let resp = match self.client.post(&url).json(&params).send().await {
            Ok(r) => r,
            Err(e) => return format!("❌ API Error: {}", e),
        };

        if !resp.status().is_success() {
            return format!("❌ Swap API returned error: {}", resp.status());
        }

        let swap_data: SwapResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => return format!("❌ Failed to parse swap data: {}", e),
        };

        // Sign and send
        let tx_bytes = match BASE64_STANDARD.decode(swap_data.transaction_base64.trim()) {
            Ok(b) => b,
            Err(e) => return format!("❌ Failed to decode transaction base64: {}", e),
        };

        let mut tx: VersionedTransaction = match bincode::deserialize(&tx_bytes) {
            Ok(t) => t,
            Err(e) => return format!("❌ Failed to deserialize transaction: {}", e),
        };

        // Manual Sign
        // Solana transactions sign the serialized message.
        let message_data = bincode::serialize(&tx.message).unwrap();
        let signature = ed25519_dalek::Signer::sign(&signing_key, &message_data);
        let solana_sig = solana_signature::Signature::from(signature.to_bytes());
        
        if !tx.signatures.is_empty() {
            tx.signatures[0] = solana_sig;
        } else {
            tx.signatures.push(solana_sig);
        }

        // Submit via RPC
        let rpc_client = self.client.clone();
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                BASE64_STANDARD.encode(bincode::serialize(&tx).unwrap()),
                { "encoding": "base64", "preflightCommitment": "confirmed" }
            ]
        });

        match rpc_client.post(&self.rpc_url).json(&body).send().await {
            Ok(r) => {
                if r.status().is_success() {
                    let r_json: Value = r.json().await.unwrap_or(json!({}));
                    let sig = r_json["result"].as_str().unwrap_or("unknown");
                    format!(
                        "✅ **Transaction Submitted!**\n\
                         Signature: `{}`\n\n\
                         🔗 [View on Solscan](https://solscan.io/tx/{})",
                        sig, sig
                    )
                } else {
                    format!("❌ RPC Error: {}", r.status())
                }
            }
            Err(e) => format!("❌ Network Error: {}", e),
        }
    }
}

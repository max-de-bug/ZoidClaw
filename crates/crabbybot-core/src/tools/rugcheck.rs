//! Solana token analysis via Rugcheck API.
//!
//! Provides token safety analysis to the agent.

use super::Tool;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error};

const RUGCHECK_API_URL: &str = "https://api.rugcheck.xyz/v1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
pub struct RugcheckFileMeta {
    pub name: String,
    pub symbol: String,
    pub image: String,
}

#[derive(Debug, Deserialize)]
pub struct RugcheckRisk {
    pub name: String,
    pub level: String,
    pub score: i32,
    pub description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RugcheckReport {
    pub score: i32,
    pub risks: Vec<RugcheckRisk>,
    pub file_meta: RugcheckFileMeta,
}

pub struct RugCheckTool {
    client: reqwest::Client,
}

impl RugCheckTool {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn fetch_report(&self, address: &str) -> Result<RugcheckReport, String> {
        let url = format!("{}/tokens/{}/report", RUGCHECK_API_URL, address);

        let response = self.client.get(&url).send().await
            .map_err(|e| format!("❌ Network error connecting to Rugcheck API: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            error!(%status, address, "Rugcheck API returned an error");
            if status.as_u16() == 404 {
                return Err(format!("❌ Token `{}` not found on Rugcheck or has no data.", address));
            }
            return Err(format!("❌ Rugcheck API error: {}", status));
        }

        response.json::<RugcheckReport>().await
            .map_err(|e| format!("❌ Failed to parse the Rugcheck report: {}", e))
    }
}

#[async_trait]
impl Tool for RugCheckTool {
    fn name(&self) -> &str {
        "rugcheck"
    }

    fn description(&self) -> &str {
        "Analyze a Solana token contract address (CA) for safety and risk factors (rug pull check). \
         Returns the token's safety score and specific risk warnings (e.g. mint authority, mutable metadata, LP lock). \
         Use this whenever a user asks to check, vet, or audit a Solana token."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "The Solana token contract address (CA) to analyze (e.g. DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263)"
                }
            },
            "required": ["address"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(address) = args.get("address").and_then(|v| v.as_str()) else {
            return "❌ Error: 'address' parameter is required".into();
        };

        if address.len() < 32 || address.len() > 44 {
            return format!("❌ Invalid address length: {}. Solana addresses are 32–44 characters.", address.len());
        }

        debug!(address, "Fetching token analysis from Rugcheck");

        let report = match self.fetch_report(address).await {
            Ok(r) => r,
            Err(e) => return e,
        };

        // Format the output
        let overall_safety = if report.score < 2000 {
            "🟢 **Good** (Low Risk)"
        } else if report.score < 5000 {
            "🟡 **Caution** (Medium Risk)"
        } else {
            "🔴 **Danger** (High Risk)"
        };

        let mut output = format!(
            "🛡️ **Rugcheck Report**\n\
             Token: **{}** (`{}`)\n\
             Address: `{}`\n\
             Score: **{}** — {}\n\n",
            report.file_meta.name, report.file_meta.symbol, address, report.score, overall_safety
        );

        if report.risks.is_empty() {
            output.push_str("✅ **No major risks detected.**");
        } else {
            output.push_str("⚠️ **Detected Risks:**\n");
            // Highlight high risks first
            let mut sorted_risks = report.risks;
            sorted_risks.sort_by(|a, b| b.score.cmp(&a.score));

            for risk in sorted_risks {
                let icon = match risk.level.as_str() {
                    "danger" => "🛑",
                    "warn" => "⚠️",
                    _ => "ℹ️",
                };
                output.push_str(&format!(
                    "{} **{}**\n   _{}_\n",
                    icon, risk.name, risk.description
                ));
            }
        }

        output.push_str(&format!(
            "\n🔗 [View Full Report on Rugcheck.xyz](https://rugcheck.xyz/tokens/{})",
            address
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rugcheck_tool() {
        let tool = RugCheckTool::new(reqwest::Client::new());
        let mut args = HashMap::new();
        args.insert(
            "address".to_string(),
            Value::String("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string()),
        );
        let result = tool.execute(args).await;
        println!("RUGCHECK RESULT:\n{}", result);
        assert!(result.contains("Score:"));
    }
}

//! The `predict` tool — full prediction pipeline entry point.
//!
//! Takes seed text and a requirement, runs the complete pipeline
//! (ontology → graph → profiles → simulation → report), and returns
//! a markdown prediction report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::info;

use crate::provider::LlmProvider;
use crate::tools::Tool;

use super::{graph_builder, ontology, profile_gen, report, simulation};
use super::types::SimulationConfig;

/// Shared state for the prediction tool, holding a reference to the
/// LLM provider so tools can make LLM calls.
pub struct PredictionState {
    pub provider: Arc<Mutex<Box<dyn LlmProvider>>>,
    pub workspace: PathBuf,
}

/// The `predict` tool runs the full prediction pipeline.
pub struct PredictTool {
    pub state: Arc<PredictionState>,
}

#[async_trait]
impl Tool for PredictTool {
    fn name(&self) -> &str {
        "predict"
    }

    fn description(&self) -> &str {
        "Run a multi-agent prediction simulation. Takes seed text (news, policies) and a \
         prediction requirement, builds a knowledge graph, simulates agent interactions, \
         and generates a structured prediction report."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The seed text to analyze (news articles, policy documents, etc.)"
                },
                "requirement": {
                    "type": "string",
                    "description": "What to predict or analyze (e.g., 'What happens to crypto prices if the SEC approves Ethereum ETFs?')"
                },
                "rounds": {
                    "type": "integer",
                    "description": "Number of simulation rounds (default: 10, max: 30)"
                },
                "max_agents": {
                    "type": "integer",
                    "description": "Maximum number of simulated agents (default: 10, max: 20)"
                }
            },
            "required": ["text", "requirement"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let requirement = args
            .get("requirement")
            .and_then(|v| v.as_str())
            .unwrap_or("General prediction");

        let rounds = args
            .get("rounds")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(30) as u32;

        let max_agents = args
            .get("max_agents")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(20) as usize;

        if text.is_empty() {
            return "Error: 'text' parameter is required and must not be empty.".into();
        }

        info!(
            text_len = text.len(),
            requirement,
            rounds,
            max_agents,
            "Starting prediction pipeline"
        );

        let provider = self.state.provider.lock().await;
        let provider_ref: &dyn LlmProvider = provider.as_ref();

        // Step 1: Generate ontology
        let ontology = match ontology::generate(provider_ref, text, requirement).await {
            Ok(o) => o,
            Err(e) => return format!("❌ Ontology generation failed: {e}"),
        };
        let step1 = format!(
            "✅ Ontology: {} entity types, {} relation types",
            ontology.entity_types.len(),
            ontology.relation_types.len()
        );

        // Step 2: Build knowledge graph
        let graph = match graph_builder::build_graph(provider_ref, text, &ontology, 500, 50).await {
            Ok(g) => g,
            Err(e) => return format!("{step1}\n❌ Graph building failed: {e}"),
        };
        let step2 = format!(
            "✅ Graph: {} entities, {} relations",
            graph.entity_count(),
            graph.relation_count()
        );

        // Save graph for later querying
        let graph_path = self.state.workspace.join("prediction_graph.json");
        if let Err(e) = graph.save(&graph_path) {
            tracing::warn!(error = %e, "Failed to save graph");
        }

        // Step 3: Generate agent profiles
        let profiles = match profile_gen::generate_profiles(provider_ref, &graph, requirement, max_agents).await {
            Ok(p) => p,
            Err(e) => return format!("{step1}\n{step2}\n❌ Profile generation failed: {e}"),
        };
        let step3 = format!("✅ Agents: {} profiles generated", profiles.len());

        // Step 4: Run simulation
        let sim_config = SimulationConfig {
            rounds,
            requirement: requirement.to_string(),
            feed_size: 5,
        };
        let sim_result = match simulation::run(provider_ref, &profiles, &graph, &sim_config).await {
            Ok(r) => r,
            Err(e) => return format!("{step1}\n{step2}\n{step3}\n❌ Simulation failed: {e}"),
        };
        let step4 = format!(
            "✅ Simulation: {} rounds, {} actions, {} posts",
            sim_result.rounds_completed, sim_result.total_actions, sim_result.posts.len()
        );

        // Step 5: Generate report
        let prediction_report = match report::generate_report(provider_ref, &graph, &sim_result, requirement).await {
            Ok(r) => r,
            Err(e) => return format!("{step1}\n{step2}\n{step3}\n{step4}\n❌ Report generation failed: {e}"),
        };

        // Return the full report with pipeline summary
        let pipeline_summary = format!(
            "---\n**Pipeline Complete**\n{step1}\n{step2}\n{step3}\n{step4}\n✅ Report generated\n---\n\n"
        );

        format!("{pipeline_summary}{}", prediction_report.to_markdown())
    }
}

//! The `simulate` tool — run a simulation on an existing knowledge graph.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::info;

use crate::provider::LlmProvider;
use crate::tools::Tool;

use super::graph::KnowledgeGraph;
use super::tool_predict::PredictionState;
use super::types::SimulationConfig;
use super::{profile_gen, report, simulation};

/// The `simulate` tool runs a simulation on an existing graph.
pub struct SimulateTool {
    pub state: Arc<PredictionState>,
}

#[async_trait]
impl Tool for SimulateTool {
    fn name(&self) -> &str {
        "simulate"
    }

    fn description(&self) -> &str {
        "Run a multi-agent simulation on an existing prediction knowledge graph. \
         Use this after `predict` or `graph_query` if you want to re-run the simulation \
         with different parameters."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "requirement": {
                    "type": "string",
                    "description": "What to simulate (e.g. 'How do crypto stakeholders react to SEC ETF approval?')"
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
            "required": ["requirement"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let requirement = args
            .get("requirement")
            .and_then(|v| v.as_str())
            .unwrap_or("General simulation");

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

        // Load existing graph
        let graph_path = self.state.workspace.join("prediction_graph.json");
        let graph = match KnowledgeGraph::load(&graph_path) {
            Ok(g) => g,
            Err(_) => {
                return "No prediction graph found. Run the `predict` tool first to build one."
                    .into();
            }
        };

        info!(
            entities = graph.entity_count(),
            rounds,
            max_agents,
            "Running simulation on existing graph"
        );

        let provider = self.state.provider.lock().await;
        let provider_ref: &dyn LlmProvider = provider.as_ref();

        // Generate profiles
        let profiles =
            match profile_gen::generate_profiles(provider_ref, &graph, requirement, max_agents)
                .await
            {
                Ok(p) => p,
                Err(e) => return format!("❌ Profile generation failed: {e}"),
            };

        // Run simulation
        let sim_config = SimulationConfig {
            rounds,
            requirement: requirement.to_string(),
            feed_size: 5,
        };

        let sim_result =
            match simulation::run(provider_ref, &profiles, &graph, &sim_config).await {
                Ok(r) => r,
                Err(e) => return format!("❌ Simulation failed: {e}"),
            };

        // Generate report
        let prediction_report =
            match report::generate_report(provider_ref, &graph, &sim_result, requirement).await {
                Ok(r) => r,
                Err(e) => return format!("❌ Report generation failed: {e}"),
            };

        let summary = format!(
            "**Simulation Complete**: {} rounds, {} actions, {} posts, {} agents\n\n",
            sim_result.rounds_completed,
            sim_result.total_actions,
            sim_result.posts.len(),
            profiles.len()
        );

        format!("{summary}{}", prediction_report.to_markdown())
    }
}

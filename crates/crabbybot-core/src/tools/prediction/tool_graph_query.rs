//! The `graph_query` tool — search the most recent knowledge graph.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

use super::graph::KnowledgeGraph;

/// The `graph_query` tool searches the prediction knowledge graph.
pub struct GraphQueryTool {
    pub workspace: PathBuf,
}

#[async_trait]
impl Tool for GraphQueryTool {
    fn name(&self) -> &str {
        "graph_query"
    }

    fn description(&self) -> &str {
        "Search the most recent prediction knowledge graph for entities and relations. \
         Useful after running a prediction to explore the extracted knowledge."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query — matches entity names (case-insensitive)"
                },
                "entity_type": {
                    "type": "string",
                    "description": "Optional: filter by entity type (e.g. 'Person', 'Organization')"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let entity_type = args
            .get("entity_type")
            .and_then(|v| v.as_str());

        let graph_path = self.workspace.join("prediction_graph.json");

        let graph = match KnowledgeGraph::load(&graph_path) {
            Ok(g) => g,
            Err(_) => {
                return "No prediction graph found. Run the `predict` tool first to build one."
                    .into();
            }
        };

        // Search by name
        let mut results = graph.find_by_name(query);

        // Optionally filter by type
        if let Some(etype) = entity_type {
            let lower = etype.to_lowercase();
            results.retain(|e| e.entity_type.to_lowercase() == lower);
        }

        if results.is_empty() {
            return format!(
                "No entities found matching '{}'. The graph has {} entities total.",
                query,
                graph.entity_count()
            );
        }

        let mut output = format!("Found {} entities matching '{}':\n\n", results.len(), query);

        for entity in results.iter().take(20) {
            output.push_str(&format!(
                "**{}** ({})\n  {}\n",
                entity.name, entity.entity_type, entity.summary
            ));

            // Show relationships
            let neighbors = graph.neighbors(&entity.id);
            if !neighbors.is_empty() {
                output.push_str("  Relationships:\n");
                for (target, relation) in neighbors.iter().take(5) {
                    output.push_str(&format!(
                        "    → {} {} ({})\n",
                        relation.relation_type, target.name, relation.fact
                    ));
                }
            }
            output.push('\n');
        }

        output
    }
}

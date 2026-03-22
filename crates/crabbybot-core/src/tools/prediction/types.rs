//! Core types for the prediction engine.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Ontology ─────────────────────────────────────────────────────────────────

/// A single entity type definition (e.g. "Person", "Organization").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTypeDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub attributes: Vec<AttributeDef>,
}

/// A single relation type definition (e.g. "INFLUENCES", "OPPOSES").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationTypeDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub source_types: Vec<String>,
    #[serde(default)]
    pub target_types: Vec<String>,
}

/// An attribute definition for entity or relation types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeDef {
    pub name: String,
    pub description: String,
}

/// The full ontology: entity types + relation types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ontology {
    pub entity_types: Vec<EntityTypeDef>,
    pub relation_types: Vec<RelationTypeDef>,
}

// ── Knowledge Graph ──────────────────────────────────────────────────────────

/// A single entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    #[serde(default)]
    pub summary: String,
}

/// A directed relation between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
    #[serde(default)]
    pub fact: String,
    #[serde(default)]
    pub weight: f64,
}

// ── Agent Profiles ───────────────────────────────────────────────────────────

/// An agent persona generated from a graph entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: usize,
    pub name: String,
    pub entity_type: String,
    pub persona: String,
    pub stance: String,
    /// 0.0 to 1.0 — how often this agent acts per round.
    pub activity_level: f64,
    #[serde(default)]
    pub traits: Vec<String>,
}

// ── Simulation ───────────────────────────────────────────────────────────────

/// A single action taken by an agent during simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub round: u32,
    pub agent_id: usize,
    pub agent_name: String,
    pub action_type: ActionType,
    pub content: String,
    /// ID of the post being replied to / liked, if applicable.
    #[serde(default)]
    pub target_post_id: Option<usize>,
}

/// The kinds of actions an agent can take.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Post,
    Reply,
    Like,
    Repost,
    Nothing,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Post => write!(f, "post"),
            Self::Reply => write!(f, "reply"),
            Self::Like => write!(f, "like"),
            Self::Repost => write!(f, "repost"),
            Self::Nothing => write!(f, "nothing"),
        }
    }
}

/// A post in the simulation's social feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimPost {
    pub id: usize,
    pub author_id: usize,
    pub author_name: String,
    pub content: String,
    pub round: u32,
    #[serde(default)]
    pub likes: u32,
    #[serde(default)]
    pub reposts: u32,
    #[serde(default)]
    pub replies: Vec<usize>,
}

/// Configuration for a simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub rounds: u32,
    pub requirement: String,
    /// Maximum posts visible to each agent per round.
    #[serde(default = "default_feed_size")]
    pub feed_size: usize,
}

fn default_feed_size() -> usize {
    5
}

/// Complete result of a simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub rounds_completed: u32,
    pub total_actions: usize,
    pub actions: Vec<AgentAction>,
    pub posts: Vec<SimPost>,
}

// ── Report ───────────────────────────────────────────────────────────────────

/// A complete prediction report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionReport {
    pub title: String,
    pub executive_summary: String,
    pub sections: Vec<ReportSection>,
    pub generated_at: String,
}

/// A section within a prediction report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSection {
    pub heading: String,
    pub content: String,
}

impl PredictionReport {
    /// Render the report as markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n", self.title);
        md.push_str(&format!("_{}_\n\n", self.generated_at));
        md.push_str("## Executive Summary\n\n");
        md.push_str(&self.executive_summary);
        md.push('\n');

        for section in &self.sections {
            md.push_str(&format!("\n## {}\n\n{}\n", section.heading, section.content));
        }

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_type_display() {
        assert_eq!(ActionType::Post.to_string(), "post");
        assert_eq!(ActionType::Reply.to_string(), "reply");
        assert_eq!(ActionType::Nothing.to_string(), "nothing");
    }

    #[test]
    fn report_to_markdown() {
        let report = PredictionReport {
            title: "Test Report".into(),
            executive_summary: "Summary here.".into(),
            sections: vec![ReportSection {
                heading: "Section 1".into(),
                content: "Content.".into(),
            }],
            generated_at: "2026-03-19".into(),
        };
        let md = report.to_markdown();
        assert!(md.contains("# Test Report"));
        assert!(md.contains("## Executive Summary"));
        assert!(md.contains("## Section 1"));
    }

    #[test]
    fn entity_serde_roundtrip() {
        let entity = Entity {
            id: "e1".into(),
            name: "Test".into(),
            entity_type: "Person".into(),
            attributes: HashMap::from([("role".into(), "CEO".into())]),
            summary: "A test entity".into(),
        };
        let json = serde_json::to_string(&entity).unwrap();
        let parsed: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "e1");
        assert_eq!(parsed.attributes["role"], "CEO");
    }
}

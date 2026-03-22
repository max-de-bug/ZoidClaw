//! In-memory knowledge graph backed by `petgraph`.
//!
//! Provides entity/relation storage with JSON persistence.
//! This replaces MiroFish's Zep Cloud dependency with a local,
//! zero-cost, portable graph.

use std::collections::HashMap;
use std::path::Path;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::types::{Entity, Relation};

/// Serialization wrapper for persisting the graph to JSON.
#[derive(Debug, Serialize, Deserialize)]
struct GraphData {
    entities: Vec<Entity>,
    relations: Vec<Relation>,
}

/// An in-memory directed knowledge graph.
///
/// Entities are nodes, relations are edges. The graph supports lookup
/// by name and type, and can be serialized to / deserialized from JSON.
pub struct KnowledgeGraph {
    graph: DiGraph<Entity, Relation>,
    /// entity_id → NodeIndex for O(1) lookup.
    index: HashMap<String, NodeIndex>,
}

impl KnowledgeGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
        }
    }

    /// Number of entities in the graph.
    pub fn entity_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of relations in the graph.
    pub fn relation_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Add an entity. If an entity with the same ID already exists,
    /// its data is updated (merge-on-write).
    pub fn add_entity(&mut self, entity: Entity) -> NodeIndex {
        if let Some(&idx) = self.index.get(&entity.id) {
            // Merge: update the existing node's data.
            if let Some(node) = self.graph.node_weight_mut(idx) {
                node.summary = entity.summary;
                for (k, v) in entity.attributes {
                    node.attributes.insert(k, v);
                }
            }
            idx
        } else {
            let id = entity.id.clone();
            let idx = self.graph.add_node(entity);
            self.index.insert(id, idx);
            idx
        }
    }

    /// Add a directed relation between two entities (by ID).
    ///
    /// Returns `true` if the relation was added, `false` if either
    /// source or target entity was not found.
    pub fn add_relation(&mut self, relation: Relation) -> bool {
        let source = self.index.get(&relation.source_id).copied();
        let target = self.index.get(&relation.target_id).copied();

        match (source, target) {
            (Some(s), Some(t)) => {
                self.graph.add_edge(s, t, relation);
                true
            }
            _ => {
                debug!(
                    source = %relation.source_id,
                    target = %relation.target_id,
                    "Skipping relation: missing entity"
                );
                false
            }
        }
    }

    /// Get an entity by its ID.
    pub fn get_entity(&self, id: &str) -> Option<&Entity> {
        self.index
            .get(id)
            .and_then(|&idx| self.graph.node_weight(idx))
    }

    /// Find entities by type (case-insensitive).
    pub fn find_by_type(&self, entity_type: &str) -> Vec<&Entity> {
        let lower = entity_type.to_lowercase();
        self.graph
            .node_weights()
            .filter(|e| e.entity_type.to_lowercase() == lower)
            .collect()
    }

    /// Find entities whose name contains the query (case-insensitive).
    pub fn find_by_name(&self, query: &str) -> Vec<&Entity> {
        let lower = query.to_lowercase();
        self.graph
            .node_weights()
            .filter(|e| e.name.to_lowercase().contains(&lower))
            .collect()
    }

    /// Get all entities in the graph.
    pub fn all_entities(&self) -> Vec<&Entity> {
        self.graph.node_weights().collect()
    }

    /// Get all relations in the graph.
    pub fn all_relations(&self) -> Vec<&Relation> {
        self.graph.edge_weights().collect()
    }

    /// Get the direct neighbors (outgoing edges) of an entity.
    pub fn neighbors(&self, entity_id: &str) -> Vec<(&Entity, &Relation)> {
        let Some(&idx) = self.index.get(entity_id) else {
            return Vec::new();
        };

        self.graph
            .edges(idx)
            .filter_map(|edge| {
                let target = self.graph.node_weight(edge.target())?;
                Some((target, edge.weight()))
            })
            .collect()
    }

    /// Serialize the graph to a JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        let data = GraphData {
            entities: self.graph.node_weights().cloned().collect(),
            relations: self.graph.edge_weights().cloned().collect(),
        };
        serde_json::to_string_pretty(&data)
    }

    /// Deserialize a graph from a JSON string.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let data: GraphData = serde_json::from_str(json)?;
        let mut graph = Self::new();

        for entity in data.entities {
            graph.add_entity(entity);
        }
        for relation in data.relations {
            graph.add_relation(relation);
        }

        Ok(graph)
    }

    /// Save the graph to a JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = self.to_json()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a graph from a JSON file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Ok(Self::from_json(&json)?)
    }

    /// Produce a concise text summary for inclusion in LLM prompts.
    pub fn summary_for_prompt(&self, max_entities: usize) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Knowledge Graph: {} entities, {} relations",
            self.entity_count(),
            self.relation_count()
        ));

        let entities: Vec<_> = self.graph.node_weights().take(max_entities).collect();
        if !entities.is_empty() {
            lines.push("Key entities:".into());
            for e in &entities {
                lines.push(format!("  - {} ({}): {}", e.name, e.entity_type, e.summary));
            }
        }

        lines.join("\n")
    }
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entity(id: &str, name: &str, etype: &str) -> Entity {
        Entity {
            id: id.into(),
            name: name.into(),
            entity_type: etype.into(),
            attributes: HashMap::new(),
            summary: format!("{name} summary"),
        }
    }

    #[test]
    fn add_and_query_entities() {
        let mut g = KnowledgeGraph::new();
        g.add_entity(make_entity("1", "Alice", "Person"));
        g.add_entity(make_entity("2", "Acme Corp", "Organization"));

        assert_eq!(g.entity_count(), 2);
        assert_eq!(g.find_by_type("Person").len(), 1);
        assert_eq!(g.find_by_name("alice").len(), 1); // case-insensitive
    }

    #[test]
    fn add_relation() {
        let mut g = KnowledgeGraph::new();
        g.add_entity(make_entity("1", "Alice", "Person"));
        g.add_entity(make_entity("2", "Bob", "Person"));

        let ok = g.add_relation(Relation {
            source_id: "1".into(),
            target_id: "2".into(),
            relation_type: "KNOWS".into(),
            fact: "Alice knows Bob".into(),
            weight: 1.0,
        });
        assert!(ok);
        assert_eq!(g.relation_count(), 1);

        let neighbors = g.neighbors("1");
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].0.name, "Bob");
    }

    #[test]
    fn missing_entity_relation_skipped() {
        let mut g = KnowledgeGraph::new();
        g.add_entity(make_entity("1", "Alice", "Person"));

        let ok = g.add_relation(Relation {
            source_id: "1".into(),
            target_id: "999".into(),
            relation_type: "KNOWS".into(),
            fact: String::new(),
            weight: 1.0,
        });
        assert!(!ok);
        assert_eq!(g.relation_count(), 0);
    }

    #[test]
    fn merge_on_duplicate_id() {
        let mut g = KnowledgeGraph::new();
        g.add_entity(make_entity("1", "Alice", "Person"));
        g.add_entity(Entity {
            id: "1".into(),
            name: "Alice".into(),
            entity_type: "Person".into(),
            attributes: HashMap::from([("role".into(), "CEO".into())]),
            summary: "Updated".into(),
        });

        assert_eq!(g.entity_count(), 1);
        let e = g.get_entity("1").unwrap();
        assert_eq!(e.summary, "Updated");
        assert_eq!(e.attributes["role"], "CEO");
    }

    #[test]
    fn json_roundtrip() {
        let mut g = KnowledgeGraph::new();
        g.add_entity(make_entity("1", "Alice", "Person"));
        g.add_entity(make_entity("2", "Bob", "Person"));
        g.add_relation(Relation {
            source_id: "1".into(),
            target_id: "2".into(),
            relation_type: "KNOWS".into(),
            fact: "friends".into(),
            weight: 1.0,
        });

        let json = g.to_json().unwrap();
        let g2 = KnowledgeGraph::from_json(&json).unwrap();

        assert_eq!(g2.entity_count(), 2);
        assert_eq!(g2.relation_count(), 1);
        assert_eq!(g2.get_entity("1").unwrap().name, "Alice");
    }
}

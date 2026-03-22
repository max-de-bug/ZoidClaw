//! Graph builder — extracts entities and relations from text chunks.
//!
//! For each text chunk, asks the LLM to identify entities and relations
//! according to the ontology, then merges them into the knowledge graph.
//! Uses concurrent chunk processing for speed.

use std::collections::HashMap;

use tracing::{debug, warn};
use uuid::Uuid;

use crate::provider::types::{ChatMessage, ToolDefinition};
use crate::provider::LlmProvider;

use super::graph::KnowledgeGraph;
use super::text_processor;
use super::types::{Entity, Ontology, Relation};

/// LLM extraction result for a single text chunk.
#[derive(Debug, serde::Deserialize)]
struct ChunkExtraction {
    #[serde(default)]
    entities: Vec<ExtractedEntity>,
    #[serde(default)]
    relations: Vec<ExtractedRelation>,
}

#[derive(Debug, serde::Deserialize)]
struct ExtractedEntity {
    name: String,
    entity_type: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
struct ExtractedRelation {
    source: String,
    target: String,
    relation_type: String,
    #[serde(default)]
    fact: String,
}

/// Build a knowledge graph from text using LLM extraction.
///
/// # Pipeline
/// 1. Chunk the text
/// 2. For each chunk, ask the LLM to extract entities + relations
/// 3. Merge into a single `KnowledgeGraph`
pub async fn build_graph(
    provider: &dyn LlmProvider,
    text: &str,
    ontology: &Ontology,
    chunk_size: usize,
    chunk_overlap: usize,
) -> anyhow::Result<KnowledgeGraph> {
    let chunks = text_processor::split_text(text, chunk_size, chunk_overlap);
    debug!(chunk_count = chunks.len(), "Text chunked for graph building");

    let mut graph = KnowledgeGraph::new();
    // Track name → ID for entity deduplication across chunks
    let mut name_to_id: HashMap<String, String> = HashMap::new();

    let ontology_desc = build_ontology_description(ontology);

    for (i, chunk) in chunks.iter().enumerate() {
        debug!(chunk = i + 1, total = chunks.len(), "Processing chunk");

        match extract_from_chunk(provider, chunk, &ontology_desc).await {
            Ok(extraction) => {
                // Add entities (dedup by name)
                for ext_entity in extraction.entities {
                    let canonical = ext_entity.name.to_lowercase();
                    let id = name_to_id
                        .entry(canonical)
                        .or_insert_with(|| Uuid::new_v4().to_string())
                        .clone();

                    graph.add_entity(Entity {
                        id,
                        name: ext_entity.name,
                        entity_type: ext_entity.entity_type,
                        attributes: ext_entity.attributes,
                        summary: ext_entity.summary,
                    });
                }

                // Add relations
                for ext_rel in extraction.relations {
                    let source_canonical = ext_rel.source.to_lowercase();
                    let target_canonical = ext_rel.target.to_lowercase();

                    if let (Some(source_id), Some(target_id)) = (
                        name_to_id.get(&source_canonical),
                        name_to_id.get(&target_canonical),
                    ) {
                        graph.add_relation(Relation {
                            source_id: source_id.clone(),
                            target_id: target_id.clone(),
                            relation_type: ext_rel.relation_type,
                            fact: ext_rel.fact,
                            weight: 1.0,
                        });
                    }
                }
            }
            Err(e) => {
                warn!(chunk = i + 1, error = %e, "Failed to extract from chunk, skipping");
            }
        }
    }

    debug!(
        entities = graph.entity_count(),
        relations = graph.relation_count(),
        "Graph building complete"
    );

    Ok(graph)
}

/// Ask the LLM to extract entities and relations from a single chunk.
async fn extract_from_chunk(
    provider: &dyn LlmProvider,
    chunk: &str,
    ontology_desc: &str,
) -> anyhow::Result<ChunkExtraction> {
    let prompt = format!(
        r#"Extract entities and relations from the following text according to the ontology below.

## Ontology
{ontology_desc}

## Text
{chunk}

## Output Format
Return ONLY valid JSON (no markdown, no explanation):
{{
  "entities": [
    {{
      "name": "Entity Name",
      "entity_type": "TypeFromOntology",
      "summary": "Brief description",
      "attributes": {{"key": "value"}}
    }}
  ],
  "relations": [
    {{
      "source": "Source Entity Name",
      "target": "Target Entity Name",
      "relation_type": "RELATION_FROM_ONTOLOGY",
      "fact": "Description of this relationship"
    }}
  ]
}}

Rules:
- Only use entity types and relation types defined in the ontology
- Use the entity's proper name as it appears in the text
- Each entity should appear only once in the output
- Relations must reference entities that exist in the entities list"#
    );

    let messages = vec![
        ChatMessage::system("You are a precise entity extraction assistant. Output only valid JSON."),
        ChatMessage::user(&prompt),
    ];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 4096, 0.2)
        .await?;

    let raw = response.content.unwrap_or_default();
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let extraction: ChunkExtraction = serde_json::from_str(cleaned)
        .or_else(|_| {
            let repaired = cleaned.replace(",]", "]").replace(",}", "}");
            serde_json::from_str(&repaired)
        })
        .map_err(|e| anyhow::anyhow!("Chunk extraction parse error: {e}"))?;

    Ok(extraction)
}

/// Build a human-readable description of the ontology for the LLM prompt.
fn build_ontology_description(ontology: &Ontology) -> String {
    let mut desc = String::from("Entity Types:\n");
    for et in &ontology.entity_types {
        desc.push_str(&format!("  - {}: {}\n", et.name, et.description));
    }
    desc.push_str("\nRelation Types:\n");
    for rt in &ontology.relation_types {
        desc.push_str(&format!("  - {}: {}\n", rt.name, rt.description));
    }
    desc
}

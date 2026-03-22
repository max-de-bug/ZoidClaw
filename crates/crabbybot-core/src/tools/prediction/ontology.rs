//! LLM-driven ontology generation.
//!
//! Asks the LLM to extract entity types and relation types from seed text,
//! mirroring MiroFish's `OntologyGenerator`.

use crate::provider::types::{ChatMessage, ToolDefinition};
use crate::provider::LlmProvider;
use tracing::{debug, warn};

use super::types::Ontology;

/// Generate an ontology from seed text using the LLM.
///
/// The LLM is prompted to identify the key entity types and relation types
/// relevant to the given text and analysis requirement. The response is
/// expected as JSON matching the [`Ontology`] schema.
pub async fn generate(
    provider: &dyn LlmProvider,
    text: &str,
    requirement: &str,
) -> anyhow::Result<Ontology> {
    let prompt = format!(
        r#"You are an ontology engineer. Analyze the following text and requirement, then produce a JSON ontology.

## Requirement
{requirement}

## Text (first 3000 chars)
{text_excerpt}

## Output Format
Return ONLY valid JSON matching this schema (no markdown, no explanation):
{{
  "entity_types": [
    {{
      "name": "EntityTypeName",
      "description": "What this entity type represents",
      "attributes": [
        {{"name": "attr_name", "description": "What this attribute captures"}}
      ]
    }}
  ],
  "relation_types": [
    {{
      "name": "RELATION_NAME",
      "description": "What this relation represents",
      "source_types": ["SourceType"],
      "target_types": ["TargetType"]
    }}
  ]
}}

Guidelines:
- Extract 5-12 entity types relevant to predicting outcomes
- Extract 5-10 relation types that capture key dynamics
- Entity types should cover: key actors, organizations, concepts, events
- Relation types should capture: influence, opposition, support, causation
- Use UPPER_SNAKE_CASE for relation names
- Keep descriptions concise (one sentence)"#,
        text_excerpt = &text[..text.len().min(3000)]
    );

    let messages = vec![
        ChatMessage::system("You are a precise JSON-generating assistant. Output only valid JSON."),
        ChatMessage::user(&prompt),
    ];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 4096, 0.3)
        .await?;

    let raw = response
        .content
        .unwrap_or_default();

    parse_ontology_json(&raw)
}

/// Parse the LLM's response into an `Ontology`, handling common issues
/// like markdown code fences or trailing commas.
fn parse_ontology_json(raw: &str) -> anyhow::Result<Ontology> {
    // Strip markdown code fences if present
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    match serde_json::from_str::<Ontology>(cleaned) {
        Ok(ontology) => {
            debug!(
                entity_types = ontology.entity_types.len(),
                relation_types = ontology.relation_types.len(),
                "Ontology parsed successfully"
            );
            Ok(ontology)
        }
        Err(e) => {
            warn!(error = %e, "Failed to parse ontology JSON, attempting repair");
            // Try removing trailing commas (common LLM mistake)
            let repaired = cleaned
                .replace(",]", "]")
                .replace(",}", "}");
            serde_json::from_str::<Ontology>(&repaired)
                .map_err(|e2| anyhow::anyhow!("Ontology JSON parse failed after repair: {e2}\nRaw: {cleaned}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_ontology() {
        let json = r#"{
            "entity_types": [
                {
                    "name": "Person",
                    "description": "A human individual",
                    "attributes": [{"name": "role", "description": "Their role"}]
                }
            ],
            "relation_types": [
                {
                    "name": "INFLUENCES",
                    "description": "One entity influences another",
                    "source_types": ["Person"],
                    "target_types": ["Person"]
                }
            ]
        }"#;
        let ontology = parse_ontology_json(json).unwrap();
        assert_eq!(ontology.entity_types.len(), 1);
        assert_eq!(ontology.relation_types.len(), 1);
    }

    #[test]
    fn parse_ontology_with_code_fence() {
        let json = "```json\n{\"entity_types\": [], \"relation_types\": []}\n```";
        let ontology = parse_ontology_json(json).unwrap();
        assert!(ontology.entity_types.is_empty());
    }

    #[test]
    fn parse_ontology_with_trailing_comma() {
        let json = r#"{"entity_types": [{"name": "X", "description": "Y", "attributes": []},], "relation_types": []}"#;
        let ontology = parse_ontology_json(json).unwrap();
        assert_eq!(ontology.entity_types.len(), 1);
    }
}

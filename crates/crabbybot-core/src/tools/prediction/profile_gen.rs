//! Agent profile generator.
//!
//! Converts knowledge graph entities into agent personas using the LLM,
//! mirroring MiroFish's `OasisProfileGenerator`.

use tracing::{debug, warn};

use crate::provider::types::{ChatMessage, ToolDefinition};
use crate::provider::LlmProvider;

use super::graph::KnowledgeGraph;
use super::types::AgentProfile;

/// Generate agent profiles from graph entities.
///
/// For each entity in the graph, asks the LLM to create a detailed persona
/// suitable for social simulation. Returns profiles sorted by activity level
/// (most active first).
pub async fn generate_profiles(
    provider: &dyn LlmProvider,
    graph: &KnowledgeGraph,
    requirement: &str,
    max_agents: usize,
) -> anyhow::Result<Vec<AgentProfile>> {
    let entities = graph.all_entities();
    if entities.is_empty() {
        return Ok(Vec::new());
    }

    // Build a context string from the graph
    let graph_summary = graph.summary_for_prompt(30);

    let mut profiles = Vec::new();

    // Process entities sequentially (LLM calls are the bottleneck anyway)
    for (i, entity) in entities.iter().enumerate().take(max_agents) {
        let neighbors = graph.neighbors(&entity.id);
        let neighbor_desc: String = neighbors
            .iter()
            .take(5)
            .map(|(n, r)| format!("  - {} ({}) via {}", n.name, n.entity_type, r.relation_type))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"Generate an agent profile for use in a social media simulation.

## Context
{graph_summary}

## Simulation Requirement
{requirement}

## Entity to Profile
Name: {name}
Type: {entity_type}
Summary: {summary}
Key Relationships:
{neighbor_desc}

## Output Format
Return ONLY valid JSON (no markdown):
{{
  "persona": "A 2-3 sentence description of this agent's personality and worldview",
  "stance": "Their position on the topic being simulated (1-2 sentences)",
  "activity_level": 0.7,
  "traits": ["trait1", "trait2", "trait3"]
}}

Guidelines:
- activity_level: 0.0 (passive observer) to 1.0 (very active poster)
- Make the persona realistic and nuanced, not a caricature
- traits: 3-5 behavioral traits relevant to social media behavior"#,
            name = entity.name,
            entity_type = entity.entity_type,
            summary = entity.summary,
        );

        let messages = vec![
            ChatMessage::system("You generate realistic agent personas as JSON."),
            ChatMessage::user(&prompt),
        ];

        match provider
            .chat(&messages, &[] as &[ToolDefinition], None, 1024, 0.5)
            .await
        {
            Ok(response) => {
                let raw = response.content.unwrap_or_default();
                match parse_profile_json(&raw, i, &entity.name, &entity.entity_type) {
                    Ok(profile) => {
                        debug!(agent = %profile.name, "Generated profile");
                        profiles.push(profile);
                    }
                    Err(e) => {
                        warn!(entity = %entity.name, error = %e, "Failed to parse profile");
                        // Fall back to a basic profile
                        profiles.push(AgentProfile {
                            id: i,
                            name: entity.name.clone(),
                            entity_type: entity.entity_type.clone(),
                            persona: format!("A {} named {}.", entity.entity_type, entity.name),
                            stance: "Neutral observer.".into(),
                            activity_level: 0.5,
                            traits: vec!["neutral".into()],
                        });
                    }
                }
            }
            Err(e) => {
                warn!(entity = %entity.name, error = %e, "LLM call failed for profile");
                profiles.push(AgentProfile {
                    id: i,
                    name: entity.name.clone(),
                    entity_type: entity.entity_type.clone(),
                    persona: format!("A {} named {}.", entity.entity_type, entity.name),
                    stance: "Neutral observer.".into(),
                    activity_level: 0.3,
                    traits: vec!["reserved".into()],
                });
            }
        }
    }

    // Sort by activity level descending (most active agents first)
    profiles.sort_by(|a, b| b.activity_level.partial_cmp(&a.activity_level).unwrap_or(std::cmp::Ordering::Equal));

    Ok(profiles)
}

/// JSON response from the LLM for a profile.
#[derive(Debug, serde::Deserialize)]
struct ProfileResponse {
    persona: String,
    stance: String,
    activity_level: f64,
    #[serde(default)]
    traits: Vec<String>,
}

fn parse_profile_json(
    raw: &str,
    id: usize,
    name: &str,
    entity_type: &str,
) -> anyhow::Result<AgentProfile> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let parsed: ProfileResponse = serde_json::from_str(cleaned)
        .or_else(|_| serde_json::from_str(&cleaned.replace(",]", "]").replace(",}", "}")))
        .map_err(|e| anyhow::anyhow!("Profile parse error: {e}"))?;

    Ok(AgentProfile {
        id,
        name: name.to_string(),
        entity_type: entity_type.to_string(),
        persona: parsed.persona,
        stance: parsed.stance,
        activity_level: parsed.activity_level.clamp(0.0, 1.0),
        traits: parsed.traits,
    })
}

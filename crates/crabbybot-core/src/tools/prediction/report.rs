//! ReACT-pattern report generation.
//!
//! Generates a structured prediction report from simulation results
//! and knowledge graph data, mirroring MiroFish's `ReportAgent`.

use tracing::{debug, warn};

use crate::provider::types::{ChatMessage, ToolDefinition};
use crate::provider::LlmProvider;

use super::graph::KnowledgeGraph;
use super::types::{PredictionReport, ReportSection, SimulationResult};

/// Generate a prediction report from simulation results.
///
/// # Pipeline
/// 1. Ask LLM to plan report sections based on the requirement
/// 2. For each section, query the graph and sim data for context
/// 3. Ask LLM to write each section
/// 4. Ask LLM to write an executive summary
pub async fn generate_report(
    provider: &dyn LlmProvider,
    graph: &KnowledgeGraph,
    sim_result: &SimulationResult,
    requirement: &str,
) -> anyhow::Result<PredictionReport> {
    let graph_summary = graph.summary_for_prompt(20);
    let sim_summary = build_sim_summary(sim_result);

    // Step 1: Plan the report
    let section_headings = plan_sections(provider, &graph_summary, &sim_summary, requirement).await?;

    // Step 2: Generate each section
    let mut sections = Vec::new();
    for heading in &section_headings {
        let content = write_section(
            provider,
            heading,
            &graph_summary,
            &sim_summary,
            requirement,
        )
        .await
        .unwrap_or_else(|e| {
            warn!(section = %heading, error = %e, "Failed to generate section");
            format!("*Section generation failed: {e}*")
        });

        sections.push(ReportSection {
            heading: heading.clone(),
            content,
        });
    }

    // Step 3: Generate executive summary
    let sections_text: String = sections
        .iter()
        .map(|s| format!("## {}\n{}", s.heading, s.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let executive_summary = write_executive_summary(provider, &sections_text, requirement)
        .await
        .unwrap_or_else(|e| format!("Executive summary generation failed: {e}"));

    // Step 4: Generate title
    let title = generate_title(provider, requirement)
        .await
        .unwrap_or_else(|_| format!("Prediction Report: {requirement}"));

    let generated_at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();

    Ok(PredictionReport {
        title,
        executive_summary,
        sections,
        generated_at,
    })
}

/// Ask the LLM to plan report section headings.
async fn plan_sections(
    provider: &dyn LlmProvider,
    graph_summary: &str,
    sim_summary: &str,
    requirement: &str,
) -> anyhow::Result<Vec<String>> {
    let prompt = format!(
        r#"Plan the sections for a prediction report.

## Requirement
{requirement}

## Available Data
{graph_summary}

## Simulation Results
{sim_summary}

Return ONLY a JSON array of section headings (3-6 sections):
["Section Heading 1", "Section Heading 2", ...]

Guidelines:
- Start with current landscape / background
- Include key findings from the simulation
- End with predictions and recommendations
- Each heading should be concise and descriptive"#
    );

    let messages = vec![
        ChatMessage::system("You plan analytical reports. Output only valid JSON arrays."),
        ChatMessage::user(&prompt),
    ];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 512, 0.3)
        .await?;

    let raw = response.content.unwrap_or_default();
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let headings: Vec<String> = serde_json::from_str(cleaned).unwrap_or_else(|_| {
        vec![
            "Current Landscape".into(),
            "Key Actors and Dynamics".into(),
            "Simulation Findings".into(),
            "Predictions and Outlook".into(),
        ]
    });

    debug!(sections = headings.len(), "Report sections planned");
    Ok(headings)
}

/// Write a single report section.
async fn write_section(
    provider: &dyn LlmProvider,
    heading: &str,
    graph_summary: &str,
    sim_summary: &str,
    requirement: &str,
) -> anyhow::Result<String> {
    let prompt = format!(
        r#"Write the "{heading}" section of a prediction report.

## Requirement
{requirement}

## Knowledge Graph Data
{graph_summary}

## Simulation Results
{sim_summary}

Write 2-4 paragraphs for this section. Be analytical and evidence-based.
Reference specific entities and simulation observations.
Do not use markdown headers (the heading is already set).
Write in a professional analyst tone."#
    );

    let messages = vec![
        ChatMessage::system("You are a senior analyst writing a prediction report section."),
        ChatMessage::user(&prompt),
    ];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 2048, 0.5)
        .await?;

    Ok(response.content.unwrap_or_default())
}

/// Write the executive summary based on all sections.
async fn write_executive_summary(
    provider: &dyn LlmProvider,
    sections_text: &str,
    requirement: &str,
) -> anyhow::Result<String> {
    let prompt = format!(
        r#"Write an executive summary for this prediction report.

## Original Requirement
{requirement}

## Report Sections
{sections_text}

Write a concise 2-3 paragraph executive summary that:
- States the key prediction clearly
- Highlights the most important findings
- Notes any significant uncertainties
- Is written for a decision-maker who may only read this section"#
    );

    let messages = vec![
        ChatMessage::system("You write concise, actionable executive summaries."),
        ChatMessage::user(&prompt),
    ];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 1024, 0.4)
        .await?;

    Ok(response.content.unwrap_or_default())
}

/// Generate a punchy report title.
async fn generate_title(
    provider: &dyn LlmProvider,
    requirement: &str,
) -> anyhow::Result<String> {
    let prompt = format!(
        "Generate a single-line title for a prediction report about: {requirement}\n\
         Return ONLY the title text, no quotes or formatting."
    );

    let messages = vec![ChatMessage::user(&prompt)];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 64, 0.5)
        .await?;

    Ok(response
        .content
        .unwrap_or_else(|| format!("Prediction: {requirement}"))
        .trim()
        .trim_matches('"')
        .to_string())
}

/// Build a text summary of simulation results for LLM context.
fn build_sim_summary(result: &SimulationResult) -> String {
    let mut lines = vec![format!(
        "Simulation: {} rounds, {} total actions, {} posts",
        result.rounds_completed, result.total_actions, result.posts.len()
    )];

    // Most active agents
    let mut action_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for action in &result.actions {
        *action_counts.entry(action.agent_name.clone()).or_default() += 1;
    }
    let mut sorted: Vec<_> = action_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    if !sorted.is_empty() {
        lines.push("Most active agents:".into());
        for (name, count) in sorted.iter().take(5) {
            lines.push(format!("  - {name}: {count} actions"));
        }
    }

    // Most popular posts
    let mut popular: Vec<_> = result.posts.iter().collect();
    popular.sort_by(|a, b| (b.likes + b.reposts).cmp(&(a.likes + a.reposts)));

    if !popular.is_empty() {
        lines.push("Most discussed posts:".into());
        for post in popular.iter().take(5) {
            lines.push(format!(
                "  - @{}: \"{}\" (❤️{}, 🔄{}, 💬{})",
                post.author_name,
                truncate(&post.content, 80),
                post.likes,
                post.reposts,
                post.replies.len(),
            ));
        }
    }

    lines.join("\n")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

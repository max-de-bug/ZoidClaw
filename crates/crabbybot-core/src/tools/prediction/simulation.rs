//! Native multi-agent simulation engine.
//!
//! A lightweight Rust replacement for OASIS. Agents take turns acting
//! in a shared social feed — posting, replying, liking, or doing nothing.
//! The LLM decides each agent's action based on their persona and
//! the current feed state.

use tracing::{debug, info, warn};

use crate::provider::types::{ChatMessage, ToolDefinition};
use crate::provider::LlmProvider;

use super::graph::KnowledgeGraph;
use super::types::{
    ActionType, AgentAction, AgentProfile, SimPost, SimulationConfig, SimulationResult,
};

/// Run a multi-agent social simulation.
///
/// Each round:
/// 1. Select active agents (based on `activity_level` probability)
/// 2. For each active agent, show them the latest posts
/// 3. Ask the LLM what action the agent takes
/// 4. Execute the action (add post, like, reply, etc.)
/// 5. Record the action
pub async fn run(
    provider: &dyn LlmProvider,
    profiles: &[AgentProfile],
    graph: &KnowledgeGraph,
    config: &SimulationConfig,
) -> anyhow::Result<SimulationResult> {
    if profiles.is_empty() {
        return Ok(SimulationResult {
            rounds_completed: 0,
            total_actions: 0,
            actions: Vec::new(),
            posts: Vec::new(),
        });
    }

    let graph_summary = graph.summary_for_prompt(15);
    let mut posts: Vec<SimPost> = Vec::new();
    let mut all_actions: Vec<AgentAction> = Vec::new();
    let mut next_post_id: usize = 1;

    info!(rounds = config.rounds, agents = profiles.len(), "Starting simulation");

    for round in 1..=config.rounds {
        debug!(round, "Simulation round");

        for profile in profiles {
            // Decide if this agent acts this round (probabilistic)
            let roll: f64 = pseudo_random(round, profile.id);
            if roll > profile.activity_level {
                continue;
            }

            // Build the agent's view of the feed
            let feed_view = build_feed_view(&posts, config.feed_size);

            let action = decide_action(
                provider,
                profile,
                &feed_view,
                &graph_summary,
                &config.requirement,
                round,
            )
            .await;

            match action {
                Ok(agent_action) => {
                    // Execute the action
                    match agent_action.action_type {
                        ActionType::Post => {
                            posts.push(SimPost {
                                id: next_post_id,
                                author_id: profile.id,
                                author_name: profile.name.clone(),
                                content: agent_action.content.clone(),
                                round,
                                likes: 0,
                                reposts: 0,
                                replies: Vec::new(),
                            });
                            next_post_id += 1;
                        }
                        ActionType::Reply => {
                            if let Some(target_id) = agent_action.target_post_id {
                                // Add reply as a new post
                                let reply_id = next_post_id;
                                posts.push(SimPost {
                                    id: reply_id,
                                    author_id: profile.id,
                                    author_name: profile.name.clone(),
                                    content: agent_action.content.clone(),
                                    round,
                                    likes: 0,
                                    reposts: 0,
                                    replies: Vec::new(),
                                });
                                next_post_id += 1;

                                // Link reply to parent
                                if let Some(parent) = posts.iter_mut().find(|p| p.id == target_id) {
                                    parent.replies.push(reply_id);
                                }
                            }
                        }
                        ActionType::Like => {
                            if let Some(target_id) = agent_action.target_post_id {
                                if let Some(post) = posts.iter_mut().find(|p| p.id == target_id) {
                                    post.likes += 1;
                                }
                            }
                        }
                        ActionType::Repost => {
                            if let Some(target_id) = agent_action.target_post_id {
                                if let Some(post) = posts.iter_mut().find(|p| p.id == target_id) {
                                    post.reposts += 1;
                                }
                            }
                        }
                        ActionType::Nothing => {}
                    }

                    all_actions.push(agent_action);
                }
                Err(e) => {
                    warn!(agent = %profile.name, round, error = %e, "Agent action failed");
                }
            }
        }
    }

    info!(
        rounds = config.rounds,
        actions = all_actions.len(),
        posts = posts.len(),
        "Simulation complete"
    );

    Ok(SimulationResult {
        rounds_completed: config.rounds,
        total_actions: all_actions.len(),
        actions: all_actions,
        posts,
    })
}

/// Ask the LLM what action this agent takes given the current feed.
async fn decide_action(
    provider: &dyn LlmProvider,
    profile: &AgentProfile,
    feed_view: &str,
    graph_summary: &str,
    requirement: &str,
    round: u32,
) -> anyhow::Result<AgentAction> {
    let prompt = format!(
        r#"You are simulating agent "{name}" in a social media prediction simulation.

## Agent Profile
Name: {name} ({entity_type})
Persona: {persona}
Stance: {stance}
Traits: {traits}

## Simulation Context
Topic: {requirement}
Round: {round}

## Knowledge
{graph_summary}

## Current Feed
{feed_view}

## Decision
What does {name} do? Return ONLY valid JSON:
{{
  "action": "post" | "reply" | "like" | "nothing",
  "content": "The text of the post/reply (empty if like/nothing)",
  "target_post_id": null | <number> (required for reply/like)
}}

Guidelines:
- Stay in character based on the persona and stance
- React naturally to existing posts
- If the feed is empty, make a post to start discussion
- Keep posts under 280 characters
- "nothing" is valid — not every agent acts every round"#,
        name = profile.name,
        entity_type = profile.entity_type,
        persona = profile.persona,
        stance = profile.stance,
        traits = profile.traits.join(", "),
    );

    let messages = vec![
        ChatMessage::system("You simulate social media agents. Output only valid JSON."),
        ChatMessage::user(&prompt),
    ];

    let response = provider
        .chat(&messages, &[] as &[ToolDefinition], None, 512, 0.7)
        .await?;

    let raw = response.content.unwrap_or_default();
    parse_action_json(&raw, profile.id, &profile.name, round)
}

/// Parse the LLM's action decision.
#[derive(Debug, serde::Deserialize)]
struct ActionResponse {
    action: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    target_post_id: Option<usize>,
}

fn parse_action_json(
    raw: &str,
    agent_id: usize,
    agent_name: &str,
    round: u32,
) -> anyhow::Result<AgentAction> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let parsed: ActionResponse = serde_json::from_str(cleaned)
        .or_else(|_| serde_json::from_str(&cleaned.replace(",}", "}")))
        .unwrap_or(ActionResponse {
            action: "nothing".into(),
            content: String::new(),
            target_post_id: None,
        });

    let action_type = match parsed.action.to_lowercase().as_str() {
        "post" => ActionType::Post,
        "reply" => ActionType::Reply,
        "like" => ActionType::Like,
        "repost" | "retweet" => ActionType::Repost,
        _ => ActionType::Nothing,
    };

    Ok(AgentAction {
        round,
        agent_id,
        agent_name: agent_name.to_string(),
        action_type,
        content: parsed.content,
        target_post_id: parsed.target_post_id,
    })
}

/// Build a text representation of the feed for the agent to see.
fn build_feed_view(posts: &[SimPost], max_posts: usize) -> String {
    if posts.is_empty() {
        return "(The feed is empty — be the first to post!)".to_string();
    }

    // Show the most recent posts
    let visible: Vec<_> = posts.iter().rev().take(max_posts).collect();

    let mut lines = Vec::new();
    for post in visible.iter().rev() {
        lines.push(format!(
            "[Post #{id}] @{author} (round {round}): {content} [❤️ {likes} | 🔄 {reposts} | 💬 {replies}]",
            id = post.id,
            author = post.author_name,
            round = post.round,
            content = post.content,
            likes = post.likes,
            reposts = post.reposts,
            replies = post.replies.len(),
        ));
    }

    lines.join("\n")
}

/// Deterministic pseudo-random number in [0, 1) for agent activation.
/// Uses a simple hash to avoid requiring `rand` in the hot path.
fn pseudo_random(round: u32, agent_id: usize) -> f64 {
    let seed = (round as u64).wrapping_mul(2654435761) ^ (agent_id as u64).wrapping_mul(1103515245);
    ((seed % 1000) as f64) / 1000.0
}

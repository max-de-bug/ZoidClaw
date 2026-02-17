//! Agent loop: the core processing engine.
//!
//! This is the heart of crabbybot. The loop:
//! 1. Receives a user message
//! 2. Builds context (system prompt + history + current message)
//! 3. Calls the LLM
//! 4. If the LLM returns tool calls → executes them → feeds results back → repeats
//! 5. When the LLM returns a final text response → returns it

pub mod context;
pub mod memory;
pub mod skills;

use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::provider::types::{
    ChatMessage, FunctionCall, ToolCallMessage,
};
use crate::provider::LlmProvider;
use crate::session::SessionManager;
use crate::tools::ToolRegistry;
use context::ContextBuilder;
use memory::MemoryStore;
use skills::SkillsLoader;

/// Configuration for the agent loop.
pub struct AgentConfig {
    pub model: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub max_iterations: u32,
    pub workspace: PathBuf,
}

/// The core agent loop.
pub struct AgentLoop {
    provider: Box<dyn LlmProvider>,
    tools: ToolRegistry,
    memory: MemoryStore,
    skills: SkillsLoader,
    sessions: SessionManager,
    config: AgentConfig,
}

impl AgentLoop {
    pub fn new(
        provider: Box<dyn LlmProvider>,
        tools: ToolRegistry,
        config: AgentConfig,
    ) -> Self {
        let memory = MemoryStore::new(&config.workspace);
        let skills = SkillsLoader::new(&config.workspace, None);
        let sessions = SessionManager::new(&config.workspace);

        Self {
            provider,
            tools,
            memory,
            skills,
            sessions,
            config,
        }
    }

    /// Process a single user message and return the agent's response.
    ///
    /// This is the main entry point. It manages the full loop of:
    /// LLM call → tool execution → LLM call → ... → final response.
    pub async fn process(
        &mut self,
        content: &str,
        session_key: &str,
    ) -> anyhow::Result<String> {
        info!(session = session_key, "Processing user message");

        // Get or create session
        let session = self.sessions.get_or_create(session_key);
        let history = session.get_history(50);

        // Build initial messages
        let ctx = ContextBuilder::new(&self.config.workspace, &self.memory, &self.skills);
        let mut messages = ctx.build_messages(&history, content, &[]);

        // Get tool definitions
        let tool_defs = self.tools.definitions();

        let mut iterations = 0;
        let max_iterations = self.config.max_iterations;

        loop {
            iterations += 1;
            if iterations > max_iterations {
                warn!(
                    iterations = max_iterations,
                    "Hit max tool iterations, forcing stop"
                );
                break;
            }

            debug!(iteration = iterations, msg_count = messages.len(), "Calling LLM");

            // Call the LLM
            let response = self
                .provider
                .chat(
                    &messages,
                    &tool_defs,
                    self.config.model.as_deref(),
                    self.config.max_tokens,
                    self.config.temperature,
                )
                .await?;

            // If no tool calls → we have our final response
            if response.tool_calls.is_empty() {
                let reply = response.content.unwrap_or_default();

                // Save to session
                let session = self.sessions.get_or_create(session_key);
                session.add_message("user", content);
                session.add_message("assistant", &reply);
                self.sessions.save(session_key)?;

                info!(
                    tokens = response.usage.total_tokens,
                    iterations,
                    "Response complete"
                );
                return Ok(reply);
            }

            // We have tool calls — add assistant message with tool calls, then execute
            let tool_call_messages: Vec<ToolCallMessage> = response
                .tool_calls
                .iter()
                .map(|tc| ToolCallMessage {
                    id: tc.id.clone(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: tc.name.clone(),
                        arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                    },
                })
                .collect();

            // Add assistant message with tool calls
            messages.push(ChatMessage::assistant_with_tool_calls(
                response.content.as_deref(),
                tool_call_messages,
            ));

            // Execute each tool call and add results
            for tc in &response.tool_calls {
                debug!(tool = tc.name, id = tc.id, "Executing tool call");

                let args: HashMap<String, serde_json::Value> = tc.arguments.clone().into_iter().collect();
                let result = self.tools.execute(&tc.name, args).await;

                debug!(
                    tool = tc.name,
                    result_len = result.len(),
                    "Tool execution complete"
                );

                messages.push(ChatMessage::tool_result(&tc.id, &tc.name, &result));
            }
        }

        // Fallback if we hit max iterations
        let session = self.sessions.get_or_create(session_key);
        session.add_message("user", content);
        session.add_message(
            "assistant",
            "I've reached the maximum number of tool iterations. Here's what I've done so far.",
        );
        self.sessions.save(session_key)?;

        Ok(
            "I've reached the maximum number of tool iterations. Please review the actions taken above."
                .into(),
        )
    }
}

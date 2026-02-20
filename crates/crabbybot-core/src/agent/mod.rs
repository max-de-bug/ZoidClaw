//! Agent loop: the core processing engine.
//!
//! This is the heart of crabbybot. The loop:
//! 1. Receives a user message
//! 2. Emits a `Typing` indicator so the channel can show a spinner
//! 3. Builds context (system prompt + token-budget history + current message)
//! 4. Calls the LLM
//! 5. If the LLM returns tool calls → executes them **concurrently** → feeds results back → repeats
//! 6. When the LLM returns a final text response → publishes `Reply` and returns

pub mod context;
pub mod memory;
pub mod skills;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures::future;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::bus::events::OutboundMessage;
use crate::bus::MessageBus;
use crate::provider::types::{ChatMessage, FunctionCall, ToolCallMessage};
use crate::provider::LlmProvider;
use crate::session::SessionManager;
use crate::tools::ToolRegistry;
use context::ContextBuilder;
use memory::MemoryStore;
use skills::SkillsLoader;

// ── Error type ────────────────────────────────────────────────────────────────

/// Typed error returned by [`AgentLoop::process`].
///
/// Callers can pattern-match to distinguish quota/rate-limit errors from
/// iteration-limit hits and generic provider failures.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// The LLM provider returned an error (network, auth, rate-limit, quota…).
    #[error("LLM provider error: {0}")]
    Provider(#[from] anyhow::Error),

    /// The agent executed `max_iterations` tool rounds without a final response.
    #[error("Max tool iterations ({0}) exceeded without a final answer")]
    MaxIterationsExceeded(u32),

    /// A session I/O error (disk full, corrupt JSONL, etc.).
    #[error("Session error: {0}")]
    Session(#[source] anyhow::Error),
}

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for the agent loop.
pub struct AgentConfig {
    pub model: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub max_iterations: u32,
    pub workspace: PathBuf,
    /// Token budget for conversation history.
    ///
    /// History will be trimmed to keep the total estimated token count
    /// (chars / 4) under this value. Defaults to 30 000 (~120 KB of text).
    pub max_context_tokens: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: None,
            max_tokens: 4096,
            temperature: 0.7,
            max_iterations: 10,
            workspace: PathBuf::from("."),
            max_context_tokens: 30_000,
        }
    }
}

// ── Agent loop ────────────────────────────────────────────────────────────────

/// The core agent loop.
///
/// Wrap in `Arc<Mutex<AgentLoop>>` to share across tasks:
/// ```ignore
/// let agent = Arc::new(Mutex::new(AgentLoop::new(provider, tools, config)));
/// ```
pub struct AgentLoop {
    provider: Box<dyn LlmProvider>,
    tools: Arc<ToolRegistry>,
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
            tools: Arc::new(tools),
            memory,
            skills,
            sessions,
            config,
        }
    }

    /// Process a single user message and return the agent's response.
    ///
    /// Publishes `Typing` and `Progress` events to `bus` during processing
    /// so channels can show real-time feedback. Pass `None` if you don't
    /// need streaming events (e.g., in tests or direct CLI usage).
    ///
    /// Returns a typed [`AgentError`] so callers can pattern-match on the
    /// failure kind.
    pub async fn process(
        &mut self,
        content: &str,
        session_key: &str,
        bus: Option<&Arc<MessageBus>>,
    ) -> Result<String, AgentError> {
        info!(session = session_key, "Processing user message");

        // ── 1. Typing indicator ───────────────────────────────────────
        let channel = session_key.split(':').next().unwrap_or("cli").to_owned();
        let chat_id = session_key
            .split_once(':')
            .map(|(_, c)| c)
            .unwrap_or("direct")
            .to_owned();

        if let Some(bus) = bus {
            bus.publish_outbound(OutboundMessage::typing(&channel, &chat_id))
                .await;
        }

        // ── 2. Build history from token budget ────────────────────────
        let session = self.sessions.get_or_create(session_key);
        let history = session.get_history_within_budget(self.config.max_context_tokens);

        // Add user message to session
        session.add_message("user", content);

        // ── 3. Build initial messages ─────────────────────────────────
        let ctx = ContextBuilder::new(&self.config.workspace, &self.memory, &self.skills);
        let mut messages = ctx.build_messages(&history, content, &[]);

        // ── 4. Tool definitions ───────────────────────────────────────
        let tool_defs = self.tools.definitions();

        let mut iterations = 0u32;
        let max_iterations = self.config.max_iterations;

        loop {
            iterations += 1;
            if iterations > max_iterations {
                warn!(iterations = max_iterations, "Hit max tool iterations, forcing stop");

                let fallback = "I've reached the maximum number of tool iterations. \
                                Please review the actions taken above.";
                {
                    let session = self.sessions.get_or_create(session_key);
                    session.add_message("assistant", fallback);
                    self.sessions
                        .save(session_key)
                        .map_err(AgentError::Session)?;
                }
                return Err(AgentError::MaxIterationsExceeded(max_iterations));
            }

            debug!(iteration = iterations, msg_count = messages.len(), "Calling LLM");

            // Emit typing again at the start of each LLM roundtrip
            if let Some(bus) = bus {
                bus.publish_outbound(OutboundMessage::typing(&channel, &chat_id))
                    .await;
            }

            // ── 5. LLM call ───────────────────────────────────────────
            let response = self
                .provider
                .chat(
                    &messages,
                    &tool_defs,
                    self.config.model.as_deref(),
                    self.config.max_tokens,
                    self.config.temperature,
                )
                .await
                .map_err(AgentError::Provider)?;

            // ── 6. Build assistant message ────────────────────────────
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

            let assistant_msg = if tool_call_messages.is_empty() {
                ChatMessage::assistant(response.content.as_deref().unwrap_or_default())
            } else {
                ChatMessage::assistant_with_tool_calls(
                    response.content.as_deref(),
                    tool_call_messages,
                )
            };

            messages.push(assistant_msg.clone());
            {
                let session = self.sessions.get_or_create(session_key);
                session.add_chat_message(&assistant_msg);
            }

            // ── 7. Final response? ────────────────────────────────────
            if response.tool_calls.is_empty() {
                let reply = response.content.unwrap_or_default();

                self.sessions
                    .save(session_key)
                    .map_err(AgentError::Session)?;

                info!(
                    tokens = response.usage.total_tokens,
                    iterations,
                    "Response complete"
                );
                return Ok(reply);
            }

            // ── 8. Concurrent tool execution ──────────────────────────
            // Emit a progress event for each tool call before launching them.
            if let Some(bus) = bus {
                let names: Vec<_> = response.tool_calls.iter().map(|tc| &tc.name).collect();
                let msg = if names.len() == 1 {
                    format!("⚙️ Running tool: `{}`…", names[0])
                } else {
                    format!(
                        "⚙️ Running {} tools in parallel: {}…",
                        names.len(),
                        names
                            .iter()
                            .map(|n| format!("`{n}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                bus.publish_outbound(OutboundMessage::progress(&channel, &chat_id, msg))
                    .await;
            }

            // Launch all tool calls concurrently; collect (id, name, result) tuples
            // and then append them in the *original order* to keep the conversation
            // schema valid (tool results must follow the matching tool calls).
            let tools = Arc::clone(&self.tools);
            let tool_futures: Vec<_> = response
                .tool_calls
                .iter()
                .map(|tc| {
                    let tools = Arc::clone(&tools);
                    let name = tc.name.clone();
                    let id = tc.id.clone();
                    let args: HashMap<String, serde_json::Value> =
                        tc.arguments.clone().into_iter().collect();

                    async move {
                        debug!(tool = %name, id = %id, "Executing tool call");
                        let result = tools.execute(&name, args).await;
                        debug!(tool = %name, result_len = result.len(), "Tool execution complete");
                        (id, name, result)
                    }
                })
                .collect();

            let results = future::join_all(tool_futures).await;

            for (id, name, result) in results {
                let tool_msg = ChatMessage::tool_result(&id, &name, &result);
                messages.push(tool_msg.clone());
                let session = self.sessions.get_or_create(session_key);
                session.add_chat_message(&tool_msg);
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ── Fake provider that answers with N tool calls then a reply ─────────────

    use crate::provider::{types::*, LlmProvider};

    struct FakeProvider {
        /// Responses to return in sequence. After exhausting them, panics.
        responses: std::sync::Mutex<std::collections::VecDeque<LlmResponse>>,
    }

    impl FakeProvider {
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses.into()),
            }
        }

        fn final_response(content: &str) -> LlmResponse {
            LlmResponse {
                content: Some(content.into()),
                tool_calls: vec![],
                finish_reason: "stop".into(),
                usage: Usage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
            }
        }

        fn tool_response(name: &str, id: &str) -> LlmResponse {
            LlmResponse {
                content: None,
                tool_calls: vec![ToolCallRequest {
                    id: id.into(),
                    name: name.into(),
                    arguments: serde_json::Map::new(),
                }],
                finish_reason: "tool_calls".into(),
                usage: Usage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
            }
        }
    }

    #[async_trait]
    impl LlmProvider for FakeProvider {
        fn default_model(&self) -> &str { "fake-model" }
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _model: Option<&str>,
            _max_tokens: u32,
            _temperature: f32,
        ) -> anyhow::Result<LlmResponse> {
            Ok(self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("FakeProvider ran out of responses"))
        }
    }


    // ── Counter tool: increments an atomic on each execution ──────────────────

    struct CounterTool {
        counter: Arc<AtomicU32>,
        name: String,
    }

    #[async_trait]
    impl Tool for CounterTool {
        fn name(&self) -> &str { &self.name }
        fn description(&self) -> &str { "counter" }
        fn parameters(&self) -> Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _args: HashMap<String, Value>) -> String {
            self.counter.fetch_add(1, Ordering::SeqCst);
            "ok".into()
        }
    }

    fn make_config(workspace: std::path::PathBuf) -> AgentConfig {
        AgentConfig {
            model: None,
            max_tokens: 100,
            temperature: 0.0,
            max_iterations: 5,
            workspace,
            max_context_tokens: 30_000,
        }
    }

    // ── Test: happy path, single turn, no tools ───────────────────────────────

    #[tokio::test]
    async fn test_simple_response() {
        let tmp = tempdir();
        let provider = FakeProvider::new(vec![FakeProvider::final_response("Hello!")]);
        let tools = ToolRegistry::new();
        let mut agent = AgentLoop::new(Box::new(provider), tools, make_config(tmp.clone()));

        let reply = agent.process("Hi", "cli:direct", None).await.unwrap();
        assert_eq!(reply, "Hello!");
    }

    // ── Test: concurrent tool execution ───────────────────────────────────────

    #[tokio::test]
    async fn test_concurrent_tool_execution() {
        let tmp = tempdir();

        // LLM: first response has two tool calls, second is a final reply
        let provider = FakeProvider::new(vec![
            LlmResponse {
                content: None,
                tool_calls: vec![
                    ToolCallRequest { id: "1".into(), name: "counter_a".into(), arguments: serde_json::Map::new() },
                    ToolCallRequest { id: "2".into(), name: "counter_b".into(), arguments: serde_json::Map::new() },
                ],
                finish_reason: "tool_calls".into(),
                usage: Usage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
            },
            FakeProvider::final_response("done"),
        ]);

        let counter_a = Arc::new(AtomicU32::new(0));
        let counter_b = Arc::new(AtomicU32::new(0));

        let mut registry = ToolRegistry::new();
        registry.register(Box::new(CounterTool { counter: Arc::clone(&counter_a), name: "counter_a".into() }));
        registry.register(Box::new(CounterTool { counter: Arc::clone(&counter_b), name: "counter_b".into() }));

        let mut agent = AgentLoop::new(Box::new(provider), registry, make_config(tmp));
        let reply = agent.process("run both", "cli:direct", None).await.unwrap();

        assert_eq!(reply, "done");
        // Both tools must have been called exactly once
        assert_eq!(counter_a.load(Ordering::SeqCst), 1, "counter_a should be called once");
        assert_eq!(counter_b.load(Ordering::SeqCst), 1, "counter_b should be called once");
    }

    // ── Test: AgentError::MaxIterationsExceeded ────────────────────────────────

    #[tokio::test]
    async fn test_max_iterations_error() {
        let tmp = tempdir();

        // Always return a tool call so we never get a final reply
        let responses: Vec<LlmResponse> = (0..10)
            .map(|i| FakeProvider::tool_response("counter_a", &i.to_string()))
            .collect();

        let provider = FakeProvider::new(responses);
        let counter = Arc::new(AtomicU32::new(0));

        let mut registry = ToolRegistry::new();
        registry.register(Box::new(CounterTool { counter: Arc::clone(&counter), name: "counter_a".into() }));

        let config = AgentConfig { max_iterations: 3, ..make_config(tmp) };
        let mut agent = AgentLoop::new(Box::new(provider), registry, config);

        let err = agent.process("loop forever", "cli:direct", None).await.unwrap_err();
        assert!(
            matches!(err, AgentError::MaxIterationsExceeded(3)),
            "expected MaxIterationsExceeded(3), got: {:?}", err
        );
    }

    // ── Test: token-budget history trimming ────────────────────────────────────

    #[tokio::test]
    async fn test_history_within_budget() {
        use crate::session::Session;

        let mut session = Session::new("test:budget");
        // Add messages totalling well over a tiny budget
        for i in 0..20 {
            session.add_message("user", &"x".repeat(100)); // 100 chars each ≈ 25 tokens
            session.add_message("assistant", &format!("Response {}", i));
        }

        // Budget of 200 tokens ≈ 800 chars → should include only last few messages
        let history = session.get_history_within_budget(200);
        assert!(!history.is_empty(), "should have some history");

        // Total estimated tokens (chars/4) must be ≤ budget
        let total_chars: usize = history
            .iter()
            .map(|m| m.content_as_str().map(|s| s.len()).unwrap_or(0))
            .sum();
        let estimated_tokens = total_chars / 4;
        assert!(
            estimated_tokens <= 200,
            "expected ≤ 200 estimated tokens, got {}",
            estimated_tokens
        );
    }

    fn tempdir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "crabbybot_test_agent_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        let _ = std::fs::create_dir_all(&path);
        path
    }
}

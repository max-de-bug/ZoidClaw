//! LLM provider trait and registry.
//!
//! Defines the `LlmProvider` trait that all backends must implement.
//! The `openai` module provides an OpenAI-compatible implementation
//! that covers most providers (OpenRouter, Anthropic, DeepSeek, Groq, vLLM, etc.).

pub mod openai;
pub mod types;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use types::{ChatMessage, LlmResponse, ToolDefinition};

/// Trait for LLM providers.
///
/// Any backend that can handle chat completions with tool calling
/// must implement this trait.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request.
    ///
    /// # Arguments
    /// * `messages` - Conversation history
    /// * `tools` - Available tool definitions (empty = no tool calling)
    /// * `model` - Model identifier override (None = use default)
    /// * `max_tokens` - Maximum response tokens
    /// * `temperature` - Sampling temperature
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        model: Option<&str>,
        max_tokens: u32,
        temperature: f32,
    ) -> anyhow::Result<LlmResponse>;

    /// Get the default model identifier.
    fn default_model(&self) -> &str;
}
/// A provider that wraps multiple other providers and implements failover logic.
///
/// If a provider returns a retryable error (like a 429), the `FallbackProvider`
/// will automatically try the next provider in its list.
pub struct FallbackProvider {
    providers: Vec<(String, Box<dyn LlmProvider>)>,
    /// Maps provider name to the time of the last transient error (e.g. 429).
    health: Mutex<HashMap<String, Instant>>,
}

/// Duration to quarantine a provider after a transient error.
const QUARANTINE_DURATION: Duration = Duration::from_secs(60);

impl FallbackProvider {
    /// Create a new fallback provider.
    pub fn new(providers: Vec<(String, Box<dyn LlmProvider>)>) -> Self {
        Self {
            providers,
            health: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl LlmProvider for FallbackProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        model: Option<&str>,
        max_tokens: u32,
        temperature: f32,
    ) -> anyhow::Result<LlmResponse> {
        let mut last_error = None;
        let now = Instant::now();

        // 1. Try healthy providers first
        for (i, (name, provider)) in self.providers.iter().enumerate() {
            let is_quarantined = {
                let health = self.health.lock().unwrap();
                health.get(name).map_or(false, |&last_err| now.duration_since(last_err) < QUARANTINE_DURATION)
            };

            if is_quarantined {
                debug!(provider = %name, "Provider is in quarantine, skipping");
                continue;
            }

            let effective_model = if i == 0 { model } else { None };

            match provider
                .chat(messages, tools, effective_model, max_tokens, temperature)
                .await
            {
                Ok(res) => return Ok(res),
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("429") || err_str.contains("quota") || err_str.contains("rate limit") {
                        warn!(
                            provider = %name,
                            error = %err_str,
                            "Provider failed with quota error, entering quarantine"
                        );
                        {
                            let mut health = self.health.lock().unwrap();
                            health.insert(name.clone(), Instant::now());
                        }
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        // 2. If all were skipped/failed, we might want to try again regardless of quarantine
        // or just return the last error. For now, we've tried all available "healthy" ones.
        // If we reach here, it means no healthy provider succeeded.

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All providers are exhausted or in quarantine")))
    }

    fn default_model(&self) -> &str {
        // Return the default model of the first provider.
        self.providers
            .first()
            .map(|(_, p)| p.default_model())
            .unwrap_or("")
    }
}

//! LLM provider trait and registry.
//!
//! Defines the `LlmProvider` trait that all backends must implement.
//! The `openai` module provides an OpenAI-compatible implementation
//! that covers most providers (OpenRouter, Anthropic, DeepSeek, Groq, vLLM, etc.).

pub mod openai;
pub mod types;

use async_trait::async_trait;
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

//! OpenAI-compatible LLM provider.
//!
//! This single implementation covers **all** providers that expose an
//! OpenAI-compatible chat completions endpoint:
//!
//! - OpenAI (`https://api.openai.com/v1`)
//! - OpenRouter (`https://openrouter.ai/api/v1`)
//! - Anthropic via OpenRouter
//! - DeepSeek (`https://api.deepseek.com/v1`)
//! - Groq (`https://api.groq.com/openai/v1`)
//! - Gemini (`https://generativelanguage.googleapis.com/v1beta/openai`)
//! - vLLM / any local server
//!
//! No LiteLLM dependency — just direct HTTP via `reqwest`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::types::{
    ChatMessage, LlmResponse, ToolCallRequest, ToolDefinition,
    Usage,
};
use super::LlmProvider;

/// Known provider base URLs.
const PROVIDER_URLS: &[(&str, &str)] = &[
    ("openrouter", "https://openrouter.ai/api/v1"),
    ("openai", "https://api.openai.com/v1"),
    ("anthropic", "https://api.anthropic.com/v1"),
    ("deepseek", "https://api.deepseek.com/v1"),
    ("groq", "https://api.groq.com/openai/v1"),
    (
        "gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
    ),
];

/// OpenAI-compatible provider that works with any provider exposing the
/// `/chat/completions` endpoint.
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    default_model: String,
}

impl OpenAiProvider {
    /// Create a new provider.
    ///
    /// # Arguments
    /// * `provider_name` - Provider identifier (e.g., "openrouter", "openai", "vllm")
    /// * `api_key` - API key for authentication
    /// * `api_base` - Custom base URL (overrides the default for the provider)
    /// * `default_model` - Default model to use
    pub fn new(
        provider_name: &str,
        api_key: &str,
        api_base: Option<&str>,
        default_model: &str,
    ) -> Self {
        let base_url = api_base
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                PROVIDER_URLS
                    .iter()
                    .find(|(name, _)| *name == provider_name)
                    .map(|(_, url)| url.to_string())
                    .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
            })
            .trim_end_matches('/')
            .to_string();

        debug!(provider = provider_name, base_url = %base_url, "Initialized LLM provider");

        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url,
            default_model: default_model.to_string(),
        }
    }
}

// ── OpenAI API request/response types ───────────────────────────────

#[derive(Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [ToolDefinition]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<UsageResponse>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageResponse,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct MessageResponse {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallResponse>>,
}

#[derive(Deserialize)]
struct ToolCallResponse {
    id: String,
    function: FunctionCallResponse,
}

#[derive(Deserialize)]
struct FunctionCallResponse {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct UsageResponse {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: String,
}

// ── LlmProvider implementation ──────────────────────────────────────

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        model: Option<&str>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<LlmResponse> {
        let model = model.unwrap_or(&self.default_model);
        let url = format!("{}/chat/completions", self.base_url);

        let tools_opt = if tools.is_empty() {
            None
        } else {
            Some(tools)
        };

        let request_body = CompletionRequest {
            model,
            messages,
            max_tokens,
            temperature,
            tools: tools_opt,
            tool_choice: if tools_opt.is_some() {
                Some("auto")
            } else {
                None
            },
        };

        debug!(model, url = %url, msg_count = messages.len(), "Sending chat completion request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to LLM API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read LLM API response body")?;

        if !status.is_success() {
            // Try to parse error message
            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
                anyhow::bail!("LLM API error ({}): {}", status, err.error.message);
            }
            anyhow::bail!("LLM API error ({}): {}", status, body);
        }

        let completion: CompletionResponse =
            serde_json::from_str(&body).context("Failed to parse LLM API response")?;

        let choice = completion
            .choices
            .into_iter()
            .next()
            .context("LLM API returned no choices")?;

        // Parse tool calls
        let tool_calls = match choice.message.tool_calls {
            Some(tcs) => tcs
                .into_iter()
                .filter_map(|tc| {
                    match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                        &tc.function.arguments,
                    ) {
                        Ok(args) => Some(ToolCallRequest {
                            id: tc.id,
                            name: tc.function.name,
                            arguments: args,
                        }),
                        Err(e) => {
                            warn!(
                                tool = tc.function.name,
                                error = %e,
                                raw = tc.function.arguments,
                                "Failed to parse tool arguments, skipping"
                            );
                            None
                        }
                    }
                })
                .collect(),
            None => Vec::new(),
        };

        let usage = completion.usage.map_or(Usage::default(), |u| Usage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        debug!(
            finish_reason = choice.finish_reason.as_deref().unwrap_or("unknown"),
            tool_calls = tool_calls.len(),
            tokens = usage.total_tokens,
            "Received LLM response"
        );

        Ok(LlmResponse {
            content: choice.message.content,
            tool_calls,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".into()),
            usage,
        })
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_url_lookup() {
        let p = OpenAiProvider::new("openrouter", "test-key", None, "test-model");
        assert_eq!(p.base_url, "https://openrouter.ai/api/v1");

        let p = OpenAiProvider::new("deepseek", "test-key", None, "test-model");
        assert_eq!(p.base_url, "https://api.deepseek.com/v1");
    }

    #[test]
    fn test_custom_base_url() {
        let p = OpenAiProvider::new(
            "vllm",
            "dummy",
            Some("http://localhost:8000/v1"),
            "llama-3",
        );
        assert_eq!(p.base_url, "http://localhost:8000/v1");
    }
}

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

/// Maximum number of retry attempts for transient errors.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (milliseconds).
const BASE_DELAY_MS: u64 = 500;

/// OpenAI-compatible provider that works with any provider exposing the
/// `/chat/completions` endpoint.
///
/// Includes automatic retry with exponential backoff for transient HTTP
/// errors (429, 500, 502, 503, 504) and network failures.
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
        client: Client,
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
            client,
            api_key: api_key.to_string(),
            base_url,
            default_model: default_model.to_string(),
        }
    }

    /// Returns `true` if the HTTP status code is transient and should be retried.
    fn is_retryable_status(status: reqwest::StatusCode) -> bool {
        matches!(
            status.as_u16(),
            429 | 500 | 502 | 503 | 504
        )
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
#[serde(untagged)]
enum ErrorResponse {
    Single(ErrorBody),
    Multiple(Vec<ErrorBody>),
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: String,
}

impl ErrorResponse {
    fn message(&self) -> String {
        match self {
            Self::Single(b) => b.error.message.clone(),
            Self::Multiple(v) => v.first().map(|b| b.error.message.clone()).unwrap_or_else(|| "Unknown error".into()),
        }
    }
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

        // ── Retry loop with exponential backoff ────────────────────
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = BASE_DELAY_MS * 2u64.pow(attempt - 1);
                warn!(attempt, delay_ms = delay, "Retrying LLM API request");
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }

            let result = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send()
                .await;

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    // Network-level errors are always retryable.
                    warn!(attempt, error = %e, "Network error calling LLM API");
                    last_error = Some(e.into());
                    continue;
                }
            };

            let status = response.status();
            let body = response
                .text()
                .await
                .context("Failed to read LLM API response body")?;

            if !status.is_success() {
                let err_msg = serde_json::from_str::<ErrorResponse>(&body)
                    .map(|e| e.message())
                    .unwrap_or_else(|_| body.clone());

                if Self::is_retryable_status(status) {
                    warn!(attempt, status = %status, "Transient LLM API error, will retry");
                    last_error = Some(anyhow::anyhow!("LLM API error ({}): {}", status, err_msg));
                    continue;
                }

                // Non-retryable error — fail immediately.
                anyhow::bail!("LLM API error ({}): {}", status, err_msg);
            }

            // ── Success path — parse the response ──────────────────
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

            return Ok(LlmResponse {
                content: choice.message.content,
                tool_calls,
                finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".into()),
                usage,
            });
        }

        // All retries exhausted.
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("LLM API request failed after {} retries", MAX_RETRIES)))
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
        let client = Client::new();
        let p = OpenAiProvider::new("openrouter", "test-key", None, "test-model", client.clone());
        assert_eq!(p.base_url, "https://openrouter.ai/api/v1");

        let p = OpenAiProvider::new("deepseek", "test-key", None, "test-model", client);
        assert_eq!(p.base_url, "https://api.deepseek.com/v1");
    }

    #[test]
    fn test_custom_base_url() {
        let p = OpenAiProvider::new(
            "vllm",
            "dummy",
            Some("http://localhost:8000/v1"),
            "llama-3",
            Client::new(),
        );
        assert_eq!(p.base_url, "http://localhost:8000/v1");
    }

    #[test]
    fn test_retryable_status() {
        assert!(OpenAiProvider::is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(OpenAiProvider::is_retryable_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(OpenAiProvider::is_retryable_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(OpenAiProvider::is_retryable_status(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(OpenAiProvider::is_retryable_status(reqwest::StatusCode::GATEWAY_TIMEOUT));

        // Non-retryable
        assert!(!OpenAiProvider::is_retryable_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(!OpenAiProvider::is_retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!OpenAiProvider::is_retryable_status(reqwest::StatusCode::NOT_FOUND));
    }
}

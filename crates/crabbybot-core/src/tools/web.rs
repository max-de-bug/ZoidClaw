//! Web tools: web_search (Brave Search API) and web_fetch (HTTP + HTML extraction).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use super::Tool;

// ── WebSearchTool ───────────────────────────────────────────────────

pub struct WebSearchTool {
    client: Client,
    api_key: String,
    max_results: u32,
}

impl WebSearchTool {
    pub fn new(api_key: &str, max_results: u32) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            max_results,
        }
    }
}

#[derive(Deserialize)]
struct BraveSearchResponse {
    web: Option<BraveWebResults>,
}

#[derive(Deserialize)]
struct BraveWebResults {
    results: Vec<BraveWebResult>,
}

#[derive(Deserialize)]
struct BraveWebResult {
    title: String,
    url: String,
    description: Option<String>,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using Brave Search API. Returns titles, URLs, and descriptions."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of results (default: 5, max: 20)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return "Error: 'query' parameter is required".into();
        };

        if self.api_key.is_empty() {
            return "Error: Brave Search API key not configured. Set tools.webSearch.apiKey in config.json".into();
        }

        let count = args
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.max_results as u64)
            .min(20);

        debug!(query, count, "Performing web search");

        let response = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("X-Subscription-Token", &self.api_key)
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<BraveSearchResponse>().await {
                    Ok(data) => {
                        let results = data
                            .web
                            .map(|w| w.results)
                            .unwrap_or_default();

                        if results.is_empty() {
                            return "No results found.".into();
                        }

                        results
                            .iter()
                            .enumerate()
                            .map(|(i, r)| {
                                let desc = r
                                    .description
                                    .as_deref()
                                    .unwrap_or("No description");
                                format!("{}. {}\n   {}\n   {}", i + 1, r.title, r.url, desc)
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    }
                    Err(e) => format!("Error parsing search results: {}", e),
                }
            }
            Ok(resp) => format!("Search API error ({})", resp.status()),
            Err(e) => format!("Search request failed: {}", e),
        }
    }
}

// ── WebFetchTool ────────────────────────────────────────────────────

pub struct WebFetchTool {
    client: Client,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a web page and extract its text content."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>) -> String {
        let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
            return "Error: 'url' parameter is required".into();
        };

        debug!(url, "Fetching web page");

        let response = self
            .client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (compatible; crabbybot/0.1; +https://github.com/crabbybot)",
            )
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(html) => extract_text_from_html(&html),
                    Err(e) => format!("Error reading response body: {}", e),
                }
            }
            Ok(resp) => format!("HTTP error: {}", resp.status()),
            Err(e) => format!("Request failed: {}", e),
        }
    }
}

/// Extract readable text from HTML using the `scraper` crate.
fn extract_text_from_html(html: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Try to find the main content area
    let selectors = ["main", "article", "body"];
    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(element) = document.select(&selector).next() {
                let text: String = element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");

                if !text.is_empty() {
                    // Truncate to ~20k chars
                    if text.len() > 20_000 {
                        return format!("{}...\n\n(truncated)", &text[..20_000]);
                    }
                    return text;
                }
            }
        }
    }

    "Could not extract text content from the page.".into()
}

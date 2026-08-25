use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    CyberSearchError, ProviderConfig, Result, SearchResult,
    model::{ProviderSearchOutput, ProviderSearchRequest},
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct GeminiProvider {
    client: Client,
    config: ProviderConfig,
}

impl GeminiProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }

    fn endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/interactions") {
            base.to_string()
        } else if base.ends_with("/v1beta") || base.ends_with("/v1beta2") {
            format!("{base}/interactions")
        } else {
            format!("{base}/v1beta/interactions")
        }
    }
}

#[async_trait]
impl SearchProvider for GeminiProvider {
    fn name(&self) -> &'static str {
        "gemini"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput> {
        let body = json!({
            "model": self.config.model.as_deref().unwrap_or("gemini-3.7-flash"),
            "input": grounded_prompt(request),
            "tools": [{"type": "google_search"}],
            "store": false
        });
        let raw = send_json(
            self.name(),
            self.client
                .post(self.endpoint())
                .header(
                    "x-goog-api-key",
                    self.config.api_key.as_deref().unwrap_or_default(),
                )
                .json(&body),
        )
        .await?;
        validate_response(&raw)?;
        Ok(ProviderSearchOutput::new(filter_results(
            parse_results(&raw),
            request,
        )))
    }
}

fn grounded_prompt(request: &ProviderSearchRequest) -> String {
    let mut prompt = format!(
        "Search Google for the following query and answer with citations from at most {} high-quality web sources: {}",
        request.limit, request.query
    );
    if !request.include_domains.is_empty() {
        prompt.push_str("\nPrefer and cite only these domains: ");
        prompt.push_str(&request.include_domains.join(", "));
    }
    if !request.exclude_domains.is_empty() {
        prompt.push_str("\nDo not cite these domains: ");
        prompt.push_str(&request.exclude_domains.join(", "));
    }
    prompt
}

fn validate_response(raw: &Value) -> Result<()> {
    if let Some(error) = raw.get("error").filter(|value| !value.is_null()) {
        return Err(CyberSearchError::provider(
            "gemini",
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Interactions API 返回错误"),
        ));
    }
    if matches!(raw.get("status").and_then(Value::as_str), Some("failed")) {
        return Err(CyberSearchError::provider(
            "gemini",
            raw.get("failure")
                .and_then(|failure| failure.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("Interaction 执行失败"),
        ));
    }
    Ok(())
}

fn parse_results(raw: &Value) -> Vec<SearchResult> {
    let mut by_url: HashMap<String, SearchResult> = HashMap::new();
    let mut order = Vec::new();
    let Some(steps) = raw.get("steps").and_then(Value::as_array) else {
        return Vec::new();
    };

    for step in steps {
        if step.get("type").and_then(Value::as_str) != Some("model_output") {
            continue;
        }
        let Some(blocks) = step.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in blocks {
            let text = block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(annotations) = block.get("annotations").and_then(Value::as_array) else {
                continue;
            };
            for annotation in annotations {
                if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
                    continue;
                }
                let url = annotation
                    .get("url")
                    .or_else(|| annotation.get("uri"))
                    .and_then(Value::as_str)
                    .filter(|url| !url.trim().is_empty());
                let Some(url) = url else {
                    continue;
                };
                let snippet = cited_segment(text, annotation);
                if let Some(existing) = by_url.get_mut(url) {
                    if snippet.as_ref().map_or(0, String::len)
                        > existing.snippet.as_ref().map_or(0, String::len)
                    {
                        existing.snippet = snippet;
                    }
                    continue;
                }
                let title = annotation
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or(url);
                order.push(url.to_string());
                by_url.insert(
                    url.to_string(),
                    SearchResult::new(title, url, snippet, None, None, "gemini"),
                );
            }
        }
    }
    order
        .into_iter()
        .filter_map(|url| by_url.remove(&url))
        .collect()
}

fn cited_segment(text: &str, annotation: &Value) -> Option<String> {
    let start = annotation
        .get("start_index")
        .or_else(|| annotation.get("startIndex"))
        .and_then(Value::as_u64)? as usize;
    let end = annotation
        .get("end_index")
        .or_else(|| annotation.get("endIndex"))
        .and_then(Value::as_u64)? as usize;
    text.get(start..end)
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_url_citations_and_keeps_order() {
        let raw = json!({
            "status": "completed",
            "steps": [
                {"type":"google_search_call","arguments":{"queries":["Rust language"]}},
                {"type":"model_output","content":[{
                    "type":"text",
                    "text":"Rust is a systems programming language.",
                    "annotations":[
                        {"type":"url_citation","start_index":0,"end_index":38,"url":"https://www.rust-lang.org/","title":"Rust"},
                        {"type":"url_citation","start_index":0,"end_index":4,"uri":"https://doc.rust-lang.org/book/","title":"The Book"},
                        {"type":"url_citation","start_index":0,"end_index":4,"url":"https://www.rust-lang.org/","title":"Duplicate"}
                    ]
                }]}
            ]
        });
        let results = parse_results(&raw);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust");
        assert_eq!(
            results[0].snippet.as_deref(),
            Some("Rust is a systems programming language")
        );
        assert_eq!(results[1].providers, ["gemini"]);
    }

    #[test]
    fn builds_endpoint_for_root_and_versioned_bases() {
        let client = Client::new();
        for (base, expected) in [
            (
                "https://example.test",
                "https://example.test/v1beta/interactions",
            ),
            (
                "https://example.test/v1beta",
                "https://example.test/v1beta/interactions",
            ),
            (
                "https://example.test/v1beta2",
                "https://example.test/v1beta2/interactions",
            ),
        ] {
            let provider = GeminiProvider::new(
                client.clone(),
                ProviderConfig {
                    name: "gemini",
                    api_key: Some("test".into()),
                    base_url: base.into(),
                    model: Some("gemini-3.7-flash".into()),
                },
            );
            assert_eq!(provider.endpoint(), expected);
        }
    }
}

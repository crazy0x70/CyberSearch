use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    ProviderConfig, Result, SearchResult,
    model::{ProviderSearchOutput, ProviderSearchRequest},
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct ExaProvider {
    client: Client,
    config: ProviderConfig,
}

impl ExaProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl SearchProvider for ExaProvider {
    fn name(&self) -> &'static str {
        "exa"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput> {
        let endpoint = format!("{}/search", self.config.base_url.trim_end_matches('/'));
        let body = json!({
            "query": request.query,
            "numResults": request.limit.min(100),
            "type": "auto",
            "includeDomains": request.include_domains,
            "excludeDomains": request.exclude_domains,
            "contents": {"highlights": {"maxCharacters": 1000}}
        });
        let raw = send_json(
            self.name(),
            self.client
                .post(endpoint)
                .header(
                    "x-api-key",
                    self.config.api_key.as_deref().unwrap_or_default(),
                )
                .json(&body),
        )
        .await?;
        Ok(ProviderSearchOutput::new(filter_results(
            parse_results(&raw),
            request,
        )))
    }
}

fn parse_results(raw: &Value) -> Vec<SearchResult> {
    raw.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("url")?.as_str()?;
            let snippet = item
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    item.get("highlights")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .filter(|text| !text.is_empty())
                })
                .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_owned));
            Some(SearchResult::new(
                item.get("title").and_then(Value::as_str).unwrap_or(url),
                url,
                snippet,
                item.get("publishedDate")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                item.get("score").and_then(Value::as_f64),
                "exa",
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exa_response() {
        let raw = json!({"results":[{"title":"Paper","url":"https://arxiv.org/abs/1","publishedDate":"2026-01-01","highlights":["one","two"]}]});
        let item = parse_results(&raw).remove(0);
        assert_eq!(item.snippet.as_deref(), Some("one\ntwo"));
        assert_eq!(item.published_at.as_deref(), Some("2026-01-01"));
    }
}

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    ProviderConfig, Result, SearchResult,
    model::{ProviderSearchOutput, ProviderSearchRequest},
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct TavilyProvider {
    client: Client,
    config: ProviderConfig,
}

impl TavilyProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl SearchProvider for TavilyProvider {
    fn name(&self) -> &'static str {
        "tavily"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput> {
        let endpoint = format!("{}/search", self.config.base_url.trim_end_matches('/'));
        let body = json!({
            "query": request.query,
            "max_results": request.limit.min(20),
            "search_depth": "basic",
            "include_answer": false,
            "include_raw_content": false,
            "include_domains": request.include_domains,
            "exclude_domains": request.exclude_domains,
        });
        let raw = send_json(
            self.name(),
            self.client
                .post(endpoint)
                .bearer_auth(self.config.api_key.as_deref().unwrap_or_default())
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
            Some(SearchResult::new(
                item.get("title").and_then(Value::as_str).unwrap_or(url),
                url,
                item.get("content")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                item.get("published_date")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                item.get("score").and_then(Value::as_f64),
                "tavily",
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tavily_response() {
        let raw = json!({"results":[{"title":"Rust","url":"https://rust-lang.org","content":"language","score":0.9}]});
        let items = parse_results(&raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].providers, ["tavily"]);
    }
}

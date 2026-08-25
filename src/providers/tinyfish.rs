use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::{
    ProviderConfig, Result, SearchResult,
    model::ProviderSearchRequest,
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct TinyFishProvider {
    client: Client,
    config: ProviderConfig,
}

impl TinyFishProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl SearchProvider for TinyFishProvider {
    fn name(&self) -> &'static str {
        "tinyfish"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<Vec<SearchResult>> {
        let mut query = vec![("query", request.query.clone())];
        if !request.include_domains.is_empty() {
            query.push(("include_domains", request.include_domains.join(",")));
        }
        if !request.exclude_domains.is_empty() {
            query.push(("exclude_domains", request.exclude_domains.join(",")));
        }
        let raw = send_json(
            self.name(),
            self.client
                .get(self.config.base_url.trim_end_matches('/'))
                .header(
                    "X-API-Key",
                    self.config.api_key.as_deref().unwrap_or_default(),
                )
                .query(&query),
        )
        .await?;
        Ok(filter_results(parse_results(&raw), request))
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
                item.get("snippet")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                item.get("date").and_then(Value::as_str).map(str::to_owned),
                item.get("position")
                    .and_then(Value::as_u64)
                    .map(|rank| 1.0 / rank.max(1) as f64),
                "tinyfish",
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_tinyfish_response() {
        let raw = json!({"results":[{"position":1,"title":"Tiny","snippet":"fast","url":"https://tinyfish.ai"}]});
        let items = parse_results(&raw);
        assert_eq!(items[0].score, Some(1.0));
    }
}

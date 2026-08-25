use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    ProviderConfig, Result, SearchResult,
    model::{ProviderSearchOutput, ProviderSearchRequest},
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct FirecrawlProvider {
    client: Client,
    config: ProviderConfig,
}

impl FirecrawlProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl SearchProvider for FirecrawlProvider {
    fn name(&self) -> &'static str {
        "firecrawl"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput> {
        let endpoint = format!("{}/v2/search", self.config.base_url.trim_end_matches('/'));
        let mut body = json!({
            "query": request.query,
            "limit": request.limit.min(100),
            "sources": ["web"],
        });
        let object = body.as_object_mut().expect("JSON object literal");
        // Firecrawl documents includeDomains and excludeDomains as mutually
        // exclusive. If both were requested, send only the allow-list and let
        // the shared local filter apply the deny-list afterwards.
        if !request.include_domains.is_empty() {
            object.insert("includeDomains".into(), json!(request.include_domains));
        } else if !request.exclude_domains.is_empty() {
            object.insert("excludeDomains".into(), json!(request.exclude_domains));
        }
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
    raw.pointer("/data/web")
        .or_else(|| raw.get("data"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("url")?.as_str()?;
            let snippet = item
                .get("description")
                .or_else(|| item.get("markdown"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(SearchResult::new(
                item.get("title").and_then(Value::as_str).unwrap_or(url),
                url,
                snippet,
                item.pointer("/metadata/publishedTime")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                None,
                "firecrawl",
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v2_firecrawl_response() {
        let raw = json!({"success":true,"data":{"web":[{"title":"Doc","url":"https://example.com","description":"text"}]}});
        let items = parse_results(&raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].snippet.as_deref(), Some("text"));
    }
}

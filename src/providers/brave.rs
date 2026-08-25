use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::{
    CyberSearchError, ProviderConfig, Result, SearchResult,
    model::{ProviderSearchOutput, ProviderSearchRequest},
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct BraveProvider {
    client: Client,
    config: ProviderConfig,
}

impl BraveProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }

    fn endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/web/search") {
            base.to_string()
        } else if base.ends_with("/res/v1") {
            format!("{base}/web/search")
        } else {
            format!("{base}/res/v1/web/search")
        }
    }
}

#[async_trait]
impl SearchProvider for BraveProvider {
    fn name(&self) -> &'static str {
        "brave"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput> {
        let query = [
            ("q", request.query.clone()),
            ("count", request.limit.min(20).to_string()),
            ("result_filter", "web".into()),
            ("text_decorations", "false".into()),
        ];
        let raw = send_json(
            self.name(),
            self.client
                .get(self.endpoint())
                .header("Accept", "application/json")
                .header(
                    "X-Subscription-Token",
                    self.config.api_key.as_deref().unwrap_or_default(),
                )
                .query(&query),
        )
        .await?;

        let results = raw
            .pointer("/web/results")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CyberSearchError::provider(
                    self.name(),
                    "Brave Search 响应缺少 web.results，本次请求不记为成功",
                )
            })?;
        if results.is_empty() {
            return Err(CyberSearchError::provider(
                self.name(),
                "Brave Search 未返回任何 web 结果",
            ));
        }

        let parsed = parse_results(&raw);
        if parsed.is_empty() {
            return Err(CyberSearchError::provider(
                self.name(),
                "Brave Search 返回了 web.results，但没有可用的结果 URL",
            ));
        }
        Ok(ProviderSearchOutput::new(filter_results(parsed, request)))
    }
}

fn parse_results(raw: &Value) -> Vec<SearchResult> {
    raw.pointer("/web/results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("url")?.as_str()?;
            let snippet = item
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    item.get("extra_snippets")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .filter(|text| !text.is_empty())
                });
            Some(SearchResult::new(
                item.get("title").and_then(Value::as_str).unwrap_or(url),
                url,
                snippet,
                item.get("age")
                    .or_else(|| item.get("page_age"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                None,
                "brave",
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(base_url: &str) -> BraveProvider {
        BraveProvider::new(
            Client::new(),
            ProviderConfig {
                name: "brave",
                api_key: Some("test".into()),
                base_url: base_url.into(),
                model: None,
            },
        )
    }

    #[test]
    fn normalizes_endpoint_shapes() {
        assert_eq!(
            provider("https://api.search.brave.com").endpoint(),
            "https://api.search.brave.com/res/v1/web/search"
        );
        assert_eq!(
            provider("https://proxy.example/res/v1").endpoint(),
            "https://proxy.example/res/v1/web/search"
        );
        assert_eq!(
            provider("https://proxy.example/res/v1/web/search").endpoint(),
            "https://proxy.example/res/v1/web/search"
        );
    }

    #[test]
    fn parses_web_results() {
        let raw = json!({"web":{"results":[{
            "title":"Brave Search",
            "url":"https://search.brave.com/",
            "description":"Independent search",
            "age":"2026-08-25T01:02:03.000Z"
        }]}});
        let results = parse_results(&raw);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].providers, ["brave"]);
        assert_eq!(
            results[0].published_at.as_deref(),
            Some("2026-08-25T01:02:03.000Z")
        );
    }
}

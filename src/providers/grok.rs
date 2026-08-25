use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Map, Value, json};

use crate::{
    CyberSearchError, ProviderConfig, Result, SearchResult,
    model::ProviderSearchRequest,
    providers::{SearchProvider, common::send_json, filter_results},
};

pub struct GrokProvider {
    client: Client,
    config: ProviderConfig,
}

impl GrokProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }

    fn endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/responses") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/responses")
        } else {
            format!("{base}/v1/responses")
        }
    }
}

#[async_trait]
impl SearchProvider for GrokProvider {
    fn name(&self) -> &'static str {
        "grok"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<Vec<SearchResult>> {
        let body = json!({
            "model": self.config.model.as_deref().unwrap_or("grok-4.6"),
            "input": [{
                "role": "user",
                "content": format!(
                    "Search the web for the following query. Use at most {} high-quality sources and cite every source: {}",
                    request.limit, request.query
                )
            }],
            "tools": [web_search_tool(request)],
            "include": ["web_search_call.action.sources"]
        });
        let raw = send_json(
            self.name(),
            self.client
                .post(self.endpoint())
                .bearer_auth(self.config.api_key.as_deref().unwrap_or_default())
                .json(&body),
        )
        .await?;
        if let Some(error) = raw.get("error").filter(|value| !value.is_null()) {
            return Err(CyberSearchError::provider(
                self.name(),
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Responses API 返回错误"),
            ));
        }
        Ok(filter_results(parse_results(&raw), request))
    }
}

fn web_search_tool(request: &ProviderSearchRequest) -> Value {
    let mut tool = Map::from_iter([("type".into(), json!("web_search"))]);
    // The upstream API accepts at most five domains and does not accept allow
    // and deny filters together. Other cases remain enforced locally after the
    // response, preserving the MCP request's final filtering semantics.
    if request.include_domains.len() <= 5 && !request.include_domains.is_empty() {
        tool.insert(
            "filters".into(),
            json!({"allowed_domains": request.include_domains}),
        );
    } else if request.include_domains.is_empty()
        && request.exclude_domains.len() <= 5
        && !request.exclude_domains.is_empty()
    {
        tool.insert(
            "filters".into(),
            json!({"excluded_domains": request.exclude_domains}),
        );
    }
    Value::Object(tool)
}

fn parse_results(raw: &Value) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    let Some(output) = raw.get("output").and_then(Value::as_array) else {
        return results;
    };

    for item in output {
        if let Some(sources) = item.pointer("/action/sources").and_then(Value::as_array) {
            for source in sources {
                push_source(&mut results, &mut seen, source);
            }
        }
        let Some(contents) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for content in contents {
            if let Some(annotations) = content.get("annotations").and_then(Value::as_array) {
                for annotation in annotations {
                    push_source(&mut results, &mut seen, annotation);
                }
            }
        }
    }
    if let Some(citations) = raw.get("citations").and_then(Value::as_array) {
        for citation in citations {
            push_source(&mut results, &mut seen, citation);
        }
    }
    results
}

fn push_source(results: &mut Vec<SearchResult>, seen: &mut HashSet<String>, source: &Value) {
    let url = source
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| source.as_str());
    let Some(url) = url.filter(|url| !url.trim().is_empty()) else {
        return;
    };
    if !seen.insert(url.to_string()) {
        return;
    }
    let title = source
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(url);
    let snippet = source
        .get("snippet")
        .or_else(|| source.get("description"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    results.push(SearchResult::new(title, url, snippet, None, None, "grok"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sources_annotations_and_citations() {
        let raw = json!({
            "output": [
                {"type":"web_search_call","action":{"sources":[
                    {"title":"Rust","url":"https://www.rust-lang.org/","snippet":"Official site"}
                ]}},
                {"type":"message","content":[{"type":"output_text","annotations":[
                    {"type":"url_citation","title":"Book","url":"https://doc.rust-lang.org/book/"},
                    {"type":"url_citation","title":"Rust duplicate","url":"https://www.rust-lang.org/"}
                ]}]}
            ],
            "citations": ["https://github.com/rust-lang/rust"]
        });
        let results = parse_results(&raw);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].snippet.as_deref(), Some("Official site"));
        assert!(results.iter().all(|item| item.providers == ["grok"]));
    }

    #[test]
    fn uses_only_supported_domain_filter_shape() {
        let request = ProviderSearchRequest {
            query: "rust".into(),
            limit: 3,
            include_domains: vec!["rust-lang.org".into()],
            exclude_domains: vec!["blog.rust-lang.org".into()],
        };
        let tool = web_search_tool(&request);
        assert_eq!(
            tool.pointer("/filters/allowed_domains/0")
                .and_then(Value::as_str),
            Some("rust-lang.org")
        );
        assert!(tool.pointer("/filters/excluded_domains").is_none());
    }
}

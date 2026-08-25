use std::collections::HashSet;

use async_trait::async_trait;
use chrono::Local;
use reqwest::Client;
use serde_json::{Map, Value, json};

use crate::{
    CyberSearchError, ProviderConfig, Result, SearchResult,
    model::{ProviderSearchOutput, ProviderSearchRequest, SearchAudit},
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

    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput> {
        let body = json!({
            "model": self.config.model.as_deref().unwrap_or("grok-4.6"),
            "input": [
                {
                    "role": "system",
                    "content": "You are a live web-search adapter. You must call the provided web_search tool before answering. Return only evidence grounded in retrieved web pages. Never answer current or time-sensitive claims from model memory."
                },
                {
                    "role": "user",
                    "content": with_current_time_context(&format!(
                        "Search the current web for this query and use at most {} high-quality sources: {}",
                        request.limit, request.query
                    ))
                }
            ],
            // web_search is the only mounted tool and required forces at least one
            // server-side tool call instead of allowing a knowledge-only answer.
            "tools": [web_search_tool(request)],
            "tool_choice": "required",
            "max_turns": 3,
            "stream": false,
            "store": false,
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
        check_api_error(&raw)?;

        let audit = web_search_audit(&raw);
        if audit.calls == 0 {
            return Err(CyberSearchError::provider(
                self.name(),
                "Responses 请求未返回 web_search_call 或 web_search 用量记录；已拒绝把模型知识回答当作联网搜索结果",
            ));
        }

        let parsed = parse_results(&raw);
        if parsed.is_empty() {
            return Err(CyberSearchError::provider(
                self.name(),
                format!(
                    "Responses 已执行 {} 次 web_search，但未返回可审计的来源 URL",
                    audit.calls
                ),
            ));
        }
        let evidence_url_count = parsed.len();
        let results = filter_results(parsed, request);
        Ok(ProviderSearchOutput::audited(
            results,
            SearchAudit {
                protocol: "xai_responses".into(),
                tool: "web_search".into(),
                tool_calls: audit.calls,
                evidence_url_count,
            },
        ))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct WebSearchAudit {
    calls: u64,
}

fn web_search_audit(raw: &Value) -> WebSearchAudit {
    let output_calls = raw
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("web_search_call"))
                .count() as u64
        })
        .unwrap_or_default();
    // Some Responses-compatible relays omit call items but retain xAI's billing
    // details. Taking the larger count avoids both false negatives and zeroed
    // usage fields while still requiring provider-generated search evidence.
    let usage_calls = raw
        .pointer("/usage/server_side_tool_usage_details/web_search_calls")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    WebSearchAudit {
        calls: output_calls.max(usage_calls),
    }
}

fn check_api_error(raw: &Value) -> Result<()> {
    if let Some(error) = raw.get("error").filter(|value| !value.is_null()) {
        return Err(CyberSearchError::provider(
            "grok",
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Grok Responses API 返回错误"),
        ));
    }
    if let Some(status @ ("failed" | "incomplete")) = raw.get("status").and_then(Value::as_str) {
        let detail = raw
            .pointer("/incomplete_details/reason")
            .or_else(|| raw.pointer("/error/message"))
            .and_then(Value::as_str)
            .unwrap_or("未提供详细原因");
        return Err(CyberSearchError::provider(
            "grok",
            format!("Responses 状态为 {status}: {detail}"),
        ));
    }
    Ok(())
}

fn with_current_time_context(query: &str) -> String {
    let now = Local::now();
    format!(
        "[Current Time Context]\n- Date: {}\n- Time: {}\n- UTC Offset: {}\nTreat this client-supplied time as the actual current time.\n\n[Search Query]\n{}",
        now.format("%Y-%m-%d"),
        now.format("%H:%M:%S"),
        now.format("%:z"),
        query
    )
}

fn web_search_tool(request: &ProviderSearchRequest) -> Value {
    let mut tool = Map::from_iter([("type".into(), json!("web_search"))]);
    // xAI allows at most five domains and does not allow both lists together.
    // Requests outside that shape are still enforced locally on returned URLs.
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

    if let Some(output) = raw.get("output").and_then(Value::as_array) {
        for item in output {
            for pointer in [
                "/action/sources",
                "/action/search_results",
                "/action/web_results",
                "/sources",
                "/results",
                "/search_results",
                "/output/sources",
                "/output/results",
            ] {
                if let Some(sources) = item.pointer(pointer).and_then(Value::as_array) {
                    for source in sources {
                        push_source(&mut results, &mut seen, source);
                    }
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
                if let Some(text) = content.get("text").and_then(Value::as_str) {
                    for (title, url) in markdown_links(text) {
                        push_source(
                            &mut results,
                            &mut seen,
                            &json!({"title": title, "url": url}),
                        );
                    }
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

fn markdown_links(text: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let mut cursor = 0;
    while let Some(relative_close) = text[cursor..].find("](") {
        let close = cursor + relative_close;
        let Some(open) = text[..close].rfind('[') else {
            cursor = close + 2;
            continue;
        };
        let url_start = close + 2;
        let Some(relative_end) = text[url_start..].find(')') else {
            break;
        };
        let url_end = url_start + relative_end;
        let title = text[open + 1..close]
            .trim_matches(['[', ']', '*', '`'])
            .trim();
        let url = text[url_start..url_end].trim();
        if url::Url::parse(url).is_ok() {
            links.push((title.to_string(), url.to_string()));
        }
        cursor = url_end + 1;
    }
    links
}

fn push_source(results: &mut Vec<SearchResult>, seen: &mut HashSet<String>, source: &Value) {
    let url = source
        .get("url")
        .or_else(|| source.get("link"))
        .and_then(Value::as_str)
        .or_else(|| source.as_str());
    let Some(url) = url.filter(|url| url::Url::parse(url).is_ok()) else {
        return;
    };
    if !seen.insert(url.to_string()) {
        return;
    }
    let title = source
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| {
            !title.trim().is_empty() && !title.chars().all(|char| char.is_ascii_digit())
        })
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
    fn parses_sources_annotations_citations_and_inline_links() {
        let raw = json!({
            "output": [
                {"type":"web_search_call","action":{"sources":[
                    {"title":"Rust","url":"https://www.rust-lang.org/","snippet":"Official site"}
                ]}},
                {"type":"message","content":[{"type":"output_text","text":"See [[1]](https://blog.rust-lang.org/).","annotations":[
                    {"type":"url_citation","title":"Book","url":"https://doc.rust-lang.org/book/"},
                    {"type":"url_citation","title":"Rust duplicate","url":"https://www.rust-lang.org/"}
                ]}]}
            ],
            "citations": ["https://github.com/rust-lang/rust"]
        });
        let results = parse_results(&raw);
        assert_eq!(results.len(), 4);
        assert_eq!(results[0].snippet.as_deref(), Some("Official site"));
        assert!(results.iter().all(|item| item.providers == ["grok"]));
    }

    #[test]
    fn counts_output_and_usage_audit_evidence() {
        let output = json!({"output":[{"type":"web_search_call"}]});
        assert_eq!(web_search_audit(&output).calls, 1);

        let relayed = json!({"usage":{"server_side_tool_usage_details":{
            "web_search_calls": 2
        }}});
        assert_eq!(web_search_audit(&relayed).calls, 2);
        assert_eq!(web_search_audit(&json!({})).calls, 0);
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

    #[test]
    fn always_uses_responses_endpoint_for_custom_gateways() {
        let provider = GrokProvider::new(
            Client::new(),
            ProviderConfig {
                name: "grok",
                api_key: Some("test".into()),
                base_url: "https://grok-gateway.example.test/v1".into(),
                model: Some("grok-4.6".into()),
            },
        );
        assert_eq!(
            provider.endpoint(),
            "https://grok-gateway.example.test/v1/responses"
        );
    }

    #[test]
    fn injects_authoritative_current_time_context() {
        let prompt = with_current_time_context("latest AI news");
        assert!(prompt.contains("[Current Time Context]"));
        assert!(prompt.contains("Treat this client-supplied time as the actual current time."));
        assert!(prompt.ends_with("latest AI news"));
    }
}

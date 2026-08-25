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
    api_mode: GrokApiMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GrokApiMode {
    Responses,
    ChatCompletions,
}

impl GrokProvider {
    pub fn new(client: Client, config: ProviderConfig, api_mode: &str) -> Self {
        let api_mode = match api_mode {
            "responses" => GrokApiMode::Responses,
            "chat_completions" => GrokApiMode::ChatCompletions,
            // Official xAI exposes the Responses web_search tool. Third-party
            // gateways are generally OpenAI Chat Completions compatible.
            _ if config
                .base_url
                .trim_end_matches('/')
                .ends_with("/responses") =>
            {
                GrokApiMode::Responses
            }
            _ if is_official_xai_base(&config.base_url) => GrokApiMode::Responses,
            _ => GrokApiMode::ChatCompletions,
        };
        Self {
            client,
            config,
            api_mode,
        }
    }

    fn responses_endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/responses") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/responses")
        } else {
            format!("{base}/v1/responses")
        }
    }

    fn chat_completions_endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    async fn search_responses(&self, request: &ProviderSearchRequest) -> Result<Vec<SearchResult>> {
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
                .post(self.responses_endpoint())
                .bearer_auth(self.config.api_key.as_deref().unwrap_or_default())
                .json(&body),
        )
        .await?;
        check_api_error(&raw)?;
        Ok(filter_results(parse_results(&raw), request))
    }

    async fn search_chat_completions(
        &self,
        request: &ProviderSearchRequest,
    ) -> Result<Vec<SearchResult>> {
        let body = json!({
            "model": self.config.model.as_deref().unwrap_or("grok-4.6"),
            "messages": [
                {
                    "role": "system",
                    "content": chat_search_prompt(request)
                },
                {
                    "role": "user",
                    "content": request.query
                }
            ],
            // Non-streaming JSON keeps the provider adapter deterministic while
            // remaining compatible with OpenAI-style Grok gateways.
            "stream": false
        });
        let raw = send_json(
            self.name(),
            self.client
                .post(self.chat_completions_endpoint())
                .bearer_auth(self.config.api_key.as_deref().unwrap_or_default())
                .json(&body),
        )
        .await?;
        check_api_error(&raw)?;
        let results = filter_results(parse_chat_results(&raw), request);
        if results.is_empty() {
            return Err(CyberSearchError::provider(
                self.name(),
                "Chat Completions 返回成功，但正文中没有可用的来源 URL",
            ));
        }
        Ok(results)
    }
}

#[async_trait]
impl SearchProvider for GrokProvider {
    fn name(&self) -> &'static str {
        "grok"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<Vec<SearchResult>> {
        match self.api_mode {
            GrokApiMode::Responses => self.search_responses(request).await,
            GrokApiMode::ChatCompletions => self.search_chat_completions(request).await,
        }
    }
}

fn is_official_xai_base(raw: &str) -> bool {
    url::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == "api.x.ai")
}

fn check_api_error(raw: &Value) -> Result<()> {
    if let Some(error) = raw.get("error").filter(|value| !value.is_null()) {
        return Err(CyberSearchError::provider(
            "grok",
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Grok API 返回错误"),
        ));
    }
    Ok(())
}

fn chat_search_prompt(request: &ProviderSearchRequest) -> String {
    let mut constraints = Vec::new();
    if !request.include_domains.is_empty() {
        constraints.push(format!(
            "Only use sources from these domains: {}.",
            request.include_domains.join(", ")
        ));
    }
    if !request.exclude_domains.is_empty() {
        constraints.push(format!(
            "Do not use sources from these domains: {}.",
            request.exclude_domains.join(", ")
        ));
    }
    format!(
        "You are a live web-search engine. Search the current web before answering. Return at most {} high-quality sources. End with a `Sources` section containing direct Markdown links in the form `- [title](https://...)`. Do not invent URLs. {}",
        request.limit,
        constraints.join(" ")
    )
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

fn parse_chat_results(raw: &Value) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    if let Some(citations) = raw.get("citations").and_then(Value::as_array) {
        for citation in citations {
            push_source(&mut results, &mut seen, citation);
        }
    }

    let Some(content) = raw
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    else {
        return results;
    };

    for (title, url) in markdown_links(content) {
        push_source(
            &mut results,
            &mut seen,
            &json!({"title": title, "url": url}),
        );
    }
    for url in bare_urls(content) {
        push_source(&mut results, &mut seen, &json!({"url": url}));
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
        let title = text[open + 1..close].trim();
        let url = text[url_start..url_end].trim();
        if !title.is_empty() && url::Url::parse(url).is_ok() {
            links.push((title.to_string(), url.to_string()));
        }
        cursor = url_end + 1;
    }
    links
}

fn bare_urls(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|token| {
            let token = token
                .trim_start_matches(['(', '[', '<', '"', '\''])
                .trim_end_matches([')', ']', '>', '"', '\'', ',', '.', ';', ':', '!', '?']);
            (token.starts_with("http://") || token.starts_with("https://"))
                .then(|| token.to_string())
        })
        .filter(|url| url::Url::parse(url).is_ok())
        .collect()
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

    #[test]
    fn auto_mode_uses_chat_completions_for_third_party_gateways() {
        let provider = GrokProvider::new(
            Client::new(),
            ProviderConfig {
                name: "grok",
                api_key: Some("test".into()),
                base_url: "https://grok-gateway.example.test/v1".into(),
                model: Some("grok-4.6".into()),
            },
            "auto",
        );
        assert_eq!(provider.api_mode, GrokApiMode::ChatCompletions);
        assert_eq!(
            provider.chat_completions_endpoint(),
            "https://grok-gateway.example.test/v1/chat/completions"
        );
    }

    #[test]
    fn auto_mode_keeps_official_xai_on_responses() {
        let provider = GrokProvider::new(
            Client::new(),
            ProviderConfig {
                name: "grok",
                api_key: Some("test".into()),
                base_url: "https://api.x.ai/v1".into(),
                model: Some("grok-4.6".into()),
            },
            "auto",
        );
        assert_eq!(provider.api_mode, GrokApiMode::Responses);
        assert_eq!(
            provider.responses_endpoint(),
            "https://api.x.ai/v1/responses"
        );
    }

    #[test]
    fn parses_chat_completion_markdown_and_bare_urls() {
        let raw = json!({
            "choices": [{
                "message": {
                    "content": "Answer.\n\nSources\n- [BBC](https://www.bbc.com/news/world)\nhttps://x.com/BBCWorld/status/123"
                }
            }]
        });
        let results = parse_chat_results(&raw);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "BBC");
        assert_eq!(results[1].url, "https://x.com/BBCWorld/status/123");
    }
}

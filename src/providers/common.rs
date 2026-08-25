use reqwest::{RequestBuilder, Response};
use serde_json::Value;
use url::Url;

use crate::{
    CyberSearchError, Result,
    model::{ProviderSearchRequest, SearchResult},
};

pub async fn send_json(provider: &str, request: RequestBuilder) -> Result<Value> {
    let response = request.send().await.map_err(|error| {
        let category = if error.is_timeout() {
            "请求超时"
        } else if error.is_connect() {
            "连接失败"
        } else if error.is_request() {
            "请求构建或发送失败"
        } else {
            "HTTP 请求失败"
        };
        CyberSearchError::provider(provider, format!("{category}: {error}"))
    })?;
    decode_json(provider, response).await
}

async fn decode_json(provider: &str, response: Response) -> Result<Value> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| CyberSearchError::provider(provider, error.to_string()))?;
    if !status.is_success() {
        if provider == "gemini" && status.as_u16() == 429 {
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|body| {
                    body.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| truncate(&text, 400));
            return Err(CyberSearchError::provider(
                provider,
                format!(
                    "HTTP 429 RESOURCE_EXHAUSTED：Gemini API 所属 Google Cloud 项目的可用配额已耗尽；请在 Google AI Studio 检查该项目的 RPM/TPM/RPD 与计费状态。上游消息: {}",
                    truncate(&message, 400)
                ),
            ));
        }
        return Err(CyberSearchError::provider(
            provider,
            format!("HTTP {}: {}", status.as_u16(), truncate(&text, 400)),
        ));
    }
    serde_json::from_str(&text).map_err(|error| {
        CyberSearchError::provider(
            provider,
            format!("响应不是有效 JSON: {error}; body={}", truncate(&text, 240)),
        )
    })
}

pub fn filter_results(
    mut results: Vec<SearchResult>,
    request: &ProviderSearchRequest,
) -> Vec<SearchResult> {
    results.retain(|item| {
        let Ok(url) = Url::parse(&item.url) else {
            return false;
        };
        let Some(host) = url.host_str().map(|host| host.to_ascii_lowercase()) else {
            return false;
        };
        let included = request.include_domains.is_empty()
            || request
                .include_domains
                .iter()
                .any(|domain| domain_matches(&host, domain));
        let excluded = request
            .exclude_domains
            .iter()
            .any(|domain| domain_matches(&host, domain));
        included && !excluded
    });
    results.truncate(request.limit);
    results
}

pub fn normalize_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    url.set_fragment(None);
    let mut pairs = url
        .query_pairs()
        .filter(|(key, _)| {
            !matches!(
                key.as_ref(),
                "utm_source"
                    | "utm_medium"
                    | "utm_campaign"
                    | "utm_term"
                    | "utm_content"
                    | "gclid"
                    | "fbclid"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.sort();
    if pairs.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    if url.path() != "/" {
        let trimmed = url.path().trim_end_matches('/').to_string();
        url.set_path(&trimmed);
    }
    Some(url.to_string())
}

fn domain_matches(host: &str, raw_domain: &str) -> bool {
    let domain = raw_domain
        .trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase();
    !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let output = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{output}…")
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_tracking_parameters_and_fragments() {
        assert_eq!(
            normalize_url("https://Example.com/docs/?utm_source=x&b=2&a=1#part").as_deref(),
            Some("https://example.com/docs?a=1&b=2")
        );
    }

    #[test]
    fn filters_subdomains() {
        let request = ProviderSearchRequest {
            query: "q".into(),
            limit: 10,
            include_domains: vec!["example.com".into()],
            exclude_domains: vec!["ads.example.com".into()],
        };
        let results = vec![
            SearchResult::new("ok", "https://docs.example.com/a", None, None, None, "test"),
            SearchResult::new("bad", "https://ads.example.com/a", None, None, None, "test"),
            SearchResult::new("other", "https://other.test/a", None, None, None, "test"),
        ];
        let filtered = filter_results(results, &request);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "ok");
    }
}

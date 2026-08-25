use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

use crate::{
    CyberSearchError, ProviderConfig, Result, SearchResult,
    model::ProviderSearchRequest,
    providers::{SearchProvider, filter_results},
};

pub struct DuckDuckGoProvider {
    client: Client,
    config: ProviderConfig,
}

impl DuckDuckGoProvider {
    pub fn new(client: Client, config: ProviderConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoProvider {
    fn name(&self) -> &'static str {
        "duckduckgo"
    }

    async fn search(&self, request: &ProviderSearchRequest) -> Result<Vec<SearchResult>> {
        let endpoint = format!("{}/html/", self.config.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .form(&[("q", request.query.as_str()), ("b", "")])
            .header("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36")
            .header("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("accept-language", "en-US,en;q=0.9")
            .header("sec-fetch-dest", "document")
            .header("sec-fetch-mode", "navigate")
            .header("sec-fetch-site", "same-origin")
            .header("sec-fetch-user", "?1")
            .header("referer", "https://html.duckduckgo.com/")
            .header("cookie", "kl=wt-wt")
            .send()
            .await
            .map_err(|error| CyberSearchError::provider(self.name(), error.to_string()))?;
        let status = response.status();
        let html = response
            .text()
            .await
            .map_err(|error| CyberSearchError::provider(self.name(), error.to_string()))?;
        if !status.is_success() {
            return Err(CyberSearchError::provider(
                self.name(),
                format!("HTTP {}", status.as_u16()),
            ));
        }
        if status.as_u16() == 202 || is_bot_challenge(&html) {
            return Err(CyberSearchError::provider(
                self.name(),
                "DuckDuckGo 返回了反自动化验证页，未获得搜索结果",
            ));
        }
        let parsed = parse_html(&html);
        if parsed.is_empty() {
            return Err(CyberSearchError::provider(
                self.name(),
                "DuckDuckGo 请求成功，但页面中没有可解析的搜索结果",
            ));
        }
        let results = filter_results(parsed, request);
        if results.is_empty() {
            return Err(CyberSearchError::provider(
                self.name(),
                "DuckDuckGo 返回了结果，但没有条目符合当前域名过滤条件",
            ));
        }
        Ok(results)
    }
}

fn is_bot_challenge(html: &str) -> bool {
    let lowercase = html.to_ascii_lowercase();
    lowercase.contains("id=\"challenge-form\"")
        || lowercase.contains("name=\"challenge-form\"")
        || lowercase.contains("duckduckgo.com/anomaly.js")
}

fn parse_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let result_selector = Selector::parse(".result").expect("static result selector");
    let title_selector = Selector::parse("a.result__a").expect("static title selector");
    let snippet_selector = Selector::parse(".result__snippet").expect("static snippet selector");

    document
        .select(&result_selector)
        .filter_map(|item| {
            let anchor = item.select(&title_selector).next()?;
            let href = anchor.value().attr("href")?;
            let url = decode_redirect(href)?;
            let title = anchor.text().collect::<String>().trim().to_string();
            let snippet = item
                .select(&snippet_selector)
                .next()
                .map(|element| element.text().collect::<String>().trim().to_string())
                .filter(|text| !text.is_empty());
            Some(SearchResult::new(
                if title.is_empty() { url.clone() } else { title },
                url,
                snippet,
                None,
                None,
                "duckduckgo",
            ))
        })
        .collect()
}

fn decode_redirect(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://duckduckgo.com{href}")
    } else {
        href.to_string()
    };
    let parsed = Url::parse(&absolute).ok()?;
    if parsed
        .host_str()
        .is_some_and(|host| host.ends_with("duckduckgo.com"))
        && let Some((_, target)) = parsed.query_pairs().find(|(key, _)| key == "uddg")
    {
        return Some(target.into_owned());
    }
    Some(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_html_results_and_redirects() {
        let html = r#"
        <div class="result">
          <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs">Example Docs</a>
          <a class="result__snippet">A useful result</a>
        </div>"#;
        let item = parse_html(html).remove(0);
        assert_eq!(item.url, "https://example.com/docs");
        assert_eq!(item.snippet.as_deref(), Some("A useful result"));
    }

    #[test]
    fn decodes_relative_redirects() {
        assert_eq!(
            decode_redirect("/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs").as_deref(),
            Some("https://example.com/docs")
        );
    }

    #[test]
    fn detects_bot_challenge_page() {
        assert!(is_bot_challenge(
            r#"<form id="challenge-form" action="//duckduckgo.com/anomaly.js"></form>"#
        ));
    }
}

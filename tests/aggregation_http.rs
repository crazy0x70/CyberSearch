use cybersearch::{Config, CyberRouter, ProviderConfig, SearchInput, SearchMode, build_providers};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, header, method, path},
};

fn provider(name: &'static str, base_url: String) -> ProviderConfig {
    ProviderConfig {
        name,
        api_key: Some(format!("{name}-secret")),
        base_url,
        model: None,
    }
}

#[tokio::test]
async fn parallel_search_merges_duplicate_urls_from_real_http_adapters() {
    let tavily = MockServer::start().await;
    let exa = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .and(header("authorization", "Bearer tavily-secret"))
        .and(body_partial_json(json!({
            "query": "rust mcp",
            "max_results": 20
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "title": "Rust MCP",
                "url": "https://example.com/mcp?utm_source=tavily",
                "content": "short",
                "score": 0.9
            }]
        })))
        .expect(1)
        .mount(&tavily)
        .await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .and(header("x-api-key", "exa-secret"))
        .and(body_partial_json(json!({
            "query": "rust mcp",
            "numResults": 20
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "title": "Official Rust MCP",
                "url": "https://example.com/mcp",
                "publishedDate": "2026-08-01",
                "highlights": ["a more complete explanation"]
            }]
        })))
        .expect(1)
        .mount(&exa)
        .await;

    let config = Config::for_test(vec![
        provider("tavily", tavily.uri()),
        provider("exa", exa.uri()),
    ]);
    let router = CyberRouter::new(&config, build_providers(&config).unwrap());
    let response = router
        .search(SearchInput {
            query: "rust mcp".into(),
            max_results: Some(10),
            providers: None,
            mode: Some(SearchMode::Parallel),
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].url, "https://example.com/mcp");
    assert_eq!(response.results[0].providers, ["tavily", "exa"]);
    assert_eq!(
        response.results[0].snippet.as_deref(),
        Some("a more complete explanation")
    );
    assert!(response.providers.iter().all(|status| status.ok));
    assert_eq!(response.fusion.pipeline, "cyber_fusion_v1");
    assert_eq!(response.fusion.received_candidates, 2);
    assert_eq!(response.fusion.collapsed_duplicates, 1);
    assert_eq!(response.fusion.consensus_results, 1);
    assert_eq!(response.fusion.successful_providers, 2);
    assert_eq!(response.fusion.failed_providers, 0);
}

#[tokio::test]
async fn fallback_search_moves_to_next_provider_after_http_error() {
    let tavily = MockServer::start().await;
    let exa = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(429).set_body_string("quota exceeded"))
        .expect(1)
        .mount(&tavily)
        .await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"title": "Backup", "url": "https://backup.example/result"}]
        })))
        .expect(1)
        .mount(&exa)
        .await;

    let config = Config::for_test(vec![
        provider("tavily", tavily.uri()),
        provider("exa", exa.uri()),
    ]);
    let router = CyberRouter::new(&config, build_providers(&config).unwrap());
    let response = router
        .search(SearchInput {
            query: "fallback".into(),
            max_results: Some(5),
            providers: None,
            mode: Some(SearchMode::Fallback),
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(response.results[0].providers, ["exa"]);
    assert_eq!(response.providers.len(), 2);
    assert!(!response.providers[0].ok);
    assert!(
        response.providers[0]
            .error
            .as_deref()
            .is_some_and(|message| message.contains("HTTP 429"))
    );
    assert!(response.providers[1].ok);
    assert_eq!(response.fusion.pipeline, "first_healthy_v1");
    assert_eq!(response.fusion.successful_providers, 1);
    assert_eq!(response.fusion.failed_providers, 1);
}

#[tokio::test]
async fn grok_search_calls_responses_api_and_returns_cited_sources() {
    let grok = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer grok-secret"))
        .and(body_partial_json(json!({
            "model": "grok-4.6",
            "tools": [{"type": "web_search"}],
            "include": ["web_search_call.action.sources"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output": [
                {
                    "type": "web_search_call",
                    "action": {
                        "sources": [
                            {
                                "title": "The Rust Programming Language",
                                "url": "https://www.rust-lang.org/",
                                "snippet": "Official Rust website"
                            }
                        ]
                    }
                }
            ]
        })))
        .expect(1)
        .mount(&grok)
        .await;

    let mut grok_config = provider("grok", grok.uri());
    grok_config.model = Some("grok-4.6".into());
    let config = Config::for_test(vec![grok_config]);
    let router = CyberRouter::new(&config, build_providers(&config).unwrap());
    let response = router
        .search(SearchInput {
            query: "Rust official website".into(),
            max_results: Some(3),
            providers: Some(vec!["grok".into()]),
            mode: Some(SearchMode::Fallback),
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].providers, ["grok"]);
    assert_eq!(response.results[0].url, "https://www.rust-lang.org/");
    assert_eq!(
        response.results[0].snippet.as_deref(),
        Some("Official Rust website")
    );
    assert_eq!(response.fusion.pipeline, "first_healthy_v1");
    assert_eq!(response.fusion.received_candidates, 1);
}

#[tokio::test]
async fn gemini_search_calls_interactions_api_and_returns_inline_citations() {
    let gemini = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1beta/interactions"))
        .and(header("x-goog-api-key", "gemini-secret"))
        .and(body_partial_json(json!({
            "model": "gemini-3.7-flash",
            "tools": [{"type": "google_search"}],
            "store": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "int_test",
            "status": "completed",
            "steps": [
                {
                    "type": "google_search_call",
                    "arguments": {"queries": ["Rust official website"]}
                },
                {
                    "type": "model_output",
                    "content": [
                        {
                            "type": "text",
                            "text": "Rust is a systems programming language.",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "start_index": 0,
                                    "end_index": 38,
                                    "url": "https://www.rust-lang.org/",
                                    "title": "Rust"
                                }
                            ]
                        }
                    ]
                }
            ]
        })))
        .expect(1)
        .mount(&gemini)
        .await;

    let mut gemini_config = provider("gemini", gemini.uri());
    gemini_config.model = Some("gemini-3.7-flash".into());
    let config = Config::for_test(vec![gemini_config]);
    let router = CyberRouter::new(&config, build_providers(&config).unwrap());
    let response = router
        .search(SearchInput {
            query: "Rust official website".into(),
            max_results: Some(3),
            providers: Some(vec!["gemini".into()]),
            mode: Some(SearchMode::Fallback),
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].providers, ["gemini"]);
    assert_eq!(response.results[0].url, "https://www.rust-lang.org/");
    assert_eq!(
        response.results[0].snippet.as_deref(),
        Some("Rust is a systems programming language")
    );
    assert_eq!(response.fusion.pipeline, "first_healthy_v1");
}

mod brave;
mod common;
mod duckduckgo;
mod exa;
mod firecrawl;
mod gemini;
mod grok;
mod tavily;
mod tinyfish;

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use reqwest::Client;

use crate::{
    config::Config,
    error::Result,
    model::{ProviderInfo, ProviderSearchOutput, ProviderSearchRequest},
};

pub(crate) use common::{filter_results, normalize_url};

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn search(&self, request: &ProviderSearchRequest) -> Result<ProviderSearchOutput>;
}

pub fn build_providers(config: &Config) -> Result<HashMap<String, Arc<dyn SearchProvider>>> {
    let build_client = |timeout| {
        Client::builder()
            .timeout(timeout)
            .user_agent(&config.user_agent)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|error| crate::CyberSearchError::Config(format!("HTTP client: {error}")))
    };
    let client = build_client(config.timeout)?;
    // Grok-compatible gateways can spend considerably longer on live browsing
    // before returning response headers. Keep that latency isolated from the
    // faster search providers instead of raising their shared timeout.
    let grok_client = build_client(config.grok_timeout)?;

    let mut providers: HashMap<String, Arc<dyn SearchProvider>> = HashMap::new();
    for name in &config.provider_order {
        let Some(item) = config.providers.get(name) else {
            continue;
        };
        if !item.enabled() {
            continue;
        }
        let provider: Arc<dyn SearchProvider> = match item.name {
            "tavily" => Arc::new(tavily::TavilyProvider::new(client.clone(), item.clone())),
            "exa" => Arc::new(exa::ExaProvider::new(client.clone(), item.clone())),
            "brave" => Arc::new(brave::BraveProvider::new(client.clone(), item.clone())),
            "firecrawl" => Arc::new(firecrawl::FirecrawlProvider::new(
                client.clone(),
                item.clone(),
            )),
            "tinyfish" => Arc::new(tinyfish::TinyFishProvider::new(
                client.clone(),
                item.clone(),
            )),
            "grok" => Arc::new(grok::GrokProvider::new(grok_client.clone(), item.clone())),
            "gemini" => Arc::new(gemini::GeminiProvider::new(client.clone(), item.clone())),
            "duckduckgo" => Arc::new(duckduckgo::DuckDuckGoProvider::new(
                client.clone(),
                item.clone(),
            )),
            _ => continue,
        };
        providers.insert(name.clone(), provider);
    }
    Ok(providers)
}

pub fn provider_info(config: &Config) -> Vec<ProviderInfo> {
    config
        .provider_order
        .iter()
        .filter_map(|name| config.providers.get(name))
        .map(|item| ProviderInfo {
            name: item.name.into(),
            enabled: item.enabled(),
            requires_api_key: item.name != "duckduckgo",
            base_url: item.base_url.clone(),
            model: item.model.clone(),
        })
        .collect()
}

use std::{collections::HashMap, env, time::Duration};

use crate::error::{CyberSearchError, Result};

pub const ALL_PROVIDERS: [&str; 7] = [
    "tavily",
    "exa",
    "firecrawl",
    "tinyfish",
    "grok",
    "gemini",
    "duckduckgo",
];

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub name: &'static str,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: Option<String>,
}

impl ProviderConfig {
    pub fn enabled(&self) -> bool {
        self.name == "duckduckgo"
            || self
                .api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub providers: HashMap<String, ProviderConfig>,
    pub provider_order: Vec<String>,
    pub timeout: Duration,
    pub grok_timeout: Duration,
    pub grok_api_mode: String,
    pub default_limit: usize,
    pub max_limit: usize,
    pub default_mode: String,
    pub user_agent: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let timeout_secs = parse_usize("CYBERSEARCH_TIMEOUT_SECONDS", 30)?;
        let grok_timeout_secs = parse_usize("GROK_TIMEOUT_SECONDS", 120)?;
        if grok_timeout_secs == 0 {
            return Err(CyberSearchError::Config(
                "GROK_TIMEOUT_SECONDS 必须大于 0".into(),
            ));
        }
        let grok_api_mode = env::var("GROK_API_MODE")
            .unwrap_or_else(|_| "auto".into())
            .trim()
            .to_ascii_lowercase();
        if !matches!(
            grok_api_mode.as_str(),
            "auto" | "responses" | "chat_completions"
        ) {
            return Err(CyberSearchError::Config(
                "GROK_API_MODE 仅支持 auto、responses 或 chat_completions".into(),
            ));
        }
        let default_limit = parse_usize("CYBERSEARCH_DEFAULT_LIMIT", 10)?;
        let max_limit = parse_usize("CYBERSEARCH_MAX_LIMIT", 30)?;
        if default_limit == 0 || max_limit == 0 || default_limit > max_limit {
            return Err(CyberSearchError::Config(
                "CYBERSEARCH_DEFAULT_LIMIT 必须大于 0 且不超过 CYBERSEARCH_MAX_LIMIT".into(),
            ));
        }

        let provider_order = env::var("CYBERSEARCH_PROVIDERS")
            .unwrap_or_else(|_| ALL_PROVIDERS.join(","))
            .split(',')
            .map(|name| name.trim().to_ascii_lowercase())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        validate_provider_names(&provider_order)?;

        let default_mode = env::var("CYBERSEARCH_MODE")
            .unwrap_or_else(|_| "parallel".into())
            .trim()
            .to_ascii_lowercase();
        if !matches!(default_mode.as_str(), "parallel" | "fallback") {
            return Err(CyberSearchError::Config(
                "CYBERSEARCH_MODE 仅支持 parallel 或 fallback".into(),
            ));
        }

        let mut providers = HashMap::new();
        providers.insert(
            "tavily".into(),
            provider(
                "tavily",
                "TAVILY_API_KEY",
                "TAVILY_BASE_URL",
                "https://api.tavily.com",
            ),
        );
        providers.insert(
            "exa".into(),
            provider("exa", "EXA_API_KEY", "EXA_BASE_URL", "https://api.exa.ai"),
        );
        providers.insert(
            "firecrawl".into(),
            provider(
                "firecrawl",
                "FIRECRAWL_API_KEY",
                "FIRECRAWL_BASE_URL",
                "https://api.firecrawl.dev",
            ),
        );
        providers.insert(
            "tinyfish".into(),
            provider(
                "tinyfish",
                "TINYFISH_API_KEY",
                "TINYFISH_BASE_URL",
                "https://api.search.tinyfish.ai",
            ),
        );
        providers.insert("grok".into(), grok_provider());
        providers.insert("gemini".into(), gemini_provider());
        providers.insert(
            "duckduckgo".into(),
            ProviderConfig {
                name: "duckduckgo",
                api_key: None,
                base_url: env::var("DUCKDUCKGO_BASE_URL")
                    .unwrap_or_else(|_| "https://html.duckduckgo.com".into()),
                model: None,
            },
        );

        Ok(Self {
            providers,
            provider_order,
            timeout: Duration::from_secs(timeout_secs as u64),
            grok_timeout: Duration::from_secs(grok_timeout_secs as u64),
            grok_api_mode,
            default_limit,
            max_limit,
            default_mode,
            user_agent: env::var("CYBERSEARCH_USER_AGENT")
                .unwrap_or_else(|_| format!("CyberSearch/{}", env!("CARGO_PKG_VERSION"))),
        })
    }

    pub fn for_test(providers: Vec<ProviderConfig>) -> Self {
        let provider_order = providers.iter().map(|item| item.name.to_string()).collect();
        Self {
            providers: providers
                .into_iter()
                .map(|item| (item.name.to_string(), item))
                .collect(),
            provider_order,
            timeout: Duration::from_secs(3),
            grok_timeout: Duration::from_secs(3),
            grok_api_mode: "auto".into(),
            default_limit: 10,
            max_limit: 30,
            default_mode: "parallel".into(),
            user_agent: "CyberSearch-Test/0.1".into(),
        }
    }

    pub fn enabled_provider_names(&self) -> Vec<String> {
        self.provider_order
            .iter()
            .filter(|name| {
                self.providers
                    .get(*name)
                    .is_some_and(ProviderConfig::enabled)
            })
            .cloned()
            .collect()
    }
}

fn parse_usize(name: &str, default: usize) -> Result<usize> {
    match env::var(name) {
        Ok(raw) => raw
            .parse::<usize>()
            .map_err(|_| CyberSearchError::Config(format!("{name} 必须是非负整数"))),
        Err(_) => Ok(default),
    }
}

fn provider(
    name: &'static str,
    key_env: &str,
    base_env: &str,
    default_base: &str,
) -> ProviderConfig {
    ProviderConfig {
        name,
        api_key: env::var(key_env).ok().filter(|key| !key.trim().is_empty()),
        base_url: env::var(base_env).unwrap_or_else(|_| default_base.into()),
        model: None,
    }
}

fn grok_provider() -> ProviderConfig {
    ProviderConfig {
        name: "grok",
        api_key: env::var("GROK_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty()),
        base_url: env::var("GROK_BASE_URL").unwrap_or_else(|_| "https://api.x.ai".into()),
        model: Some(env::var("GROK_MODEL").unwrap_or_else(|_| "grok-4.6".into())),
    }
}

fn gemini_provider() -> ProviderConfig {
    ProviderConfig {
        name: "gemini",
        api_key: env::var("GEMINI_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty()),
        base_url: env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".into()),
        model: Some(env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.7-flash".into())),
    }
}

pub fn validate_provider_names(names: &[String]) -> Result<()> {
    for name in names {
        if !ALL_PROVIDERS.contains(&name.as_str()) {
            return Err(CyberSearchError::UnknownProvider(name.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_provider_names() {
        assert!(
            validate_provider_names(&["tavily".into(), "grok".into(), "gemini".into()]).is_ok()
        );
        assert!(matches!(
            validate_provider_names(&["unknown".into()]),
            Err(CyberSearchError::UnknownProvider(_))
        ));
    }
}

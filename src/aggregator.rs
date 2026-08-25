use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use futures::{StreamExt, stream::FuturesUnordered};

use crate::{
    Config, CyberSearchError, Result, SearchProvider,
    config::validate_provider_names,
    fusion::{FusionOutput, RankedBatch, cyber_fuse},
    model::{
        AggregateSearchResponse, ProviderSearchRequest, ProviderStatus, SearchInput, SearchMode,
    },
};

#[derive(Clone)]
pub struct CyberRouter {
    providers: HashMap<String, Arc<dyn SearchProvider>>,
    provider_order: Vec<String>,
    default_limit: usize,
    max_limit: usize,
    default_mode: SearchMode,
}

impl CyberRouter {
    pub fn new(config: &Config, providers: HashMap<String, Arc<dyn SearchProvider>>) -> Self {
        Self {
            providers,
            provider_order: config.provider_order.clone(),
            default_limit: config.default_limit,
            max_limit: config.max_limit,
            default_mode: if config.default_mode == "fallback" {
                SearchMode::Fallback
            } else {
                SearchMode::Parallel
            },
        }
    }

    pub async fn search(&self, input: SearchInput) -> Result<AggregateSearchResponse> {
        let query = input.query.trim().to_string();
        if query.is_empty() {
            return Err(CyberSearchError::Config("query 不能为空".into()));
        }
        let limit = input
            .max_results
            .unwrap_or(self.default_limit)
            .clamp(1, self.max_limit);
        let mode = input.mode.unwrap_or(self.default_mode);
        let selected_names = self.select_provider_names(input.providers)?;
        let request = ProviderSearchRequest {
            query: query.clone(),
            // Over-fetch gives CyberFusion and local domain filtering enough candidates.
            limit: (limit * 2).min(self.max_limit),
            include_domains: normalize_domains(input.include_domains),
            exclude_domains: normalize_domains(input.exclude_domains),
        };

        let (mut fused, statuses) = match mode {
            SearchMode::Parallel => self.parallel_search(&selected_names, &request).await?,
            SearchMode::Fallback => self.fallback_search(&selected_names, &request).await?,
        };
        fused.results.truncate(limit);
        fused.diagnostics.successful_providers = statuses.iter().filter(|status| status.ok).count();
        fused.diagnostics.failed_providers = statuses.iter().filter(|status| !status.ok).count();
        Ok(AggregateSearchResponse {
            query,
            mode,
            results: fused.results,
            providers: statuses,
            fusion: fused.diagnostics,
        })
    }

    fn select_provider_names(&self, requested: Option<Vec<String>>) -> Result<Vec<String>> {
        let names = requested
            .map(|items| {
                items
                    .into_iter()
                    .map(|name| name.trim().to_ascii_lowercase())
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| self.provider_order.clone());
        validate_provider_names(&names)?;
        let mut seen = HashSet::new();
        let names = names
            .into_iter()
            .filter(|name| seen.insert(name.clone()))
            .filter(|name| self.providers.contains_key(name))
            .collect::<Vec<_>>();
        if names.is_empty() {
            let configured = self
                .providers
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(CyberSearchError::Config(format!(
                "所选供应商均未启用；当前已启用: {}",
                if configured.is_empty() {
                    "无"
                } else {
                    &configured
                }
            )));
        }
        Ok(names)
    }

    async fn parallel_search(
        &self,
        names: &[String],
        request: &ProviderSearchRequest,
    ) -> Result<(FusionOutput, Vec<ProviderStatus>)> {
        let mut tasks = FuturesUnordered::new();
        for (order, name) in names.iter().enumerate() {
            let provider = Arc::clone(self.providers.get(name).expect("selected provider exists"));
            let request = request.clone();
            let name = name.clone();
            tasks.push(async move {
                let started = Instant::now();
                let result = provider.search(&request).await;
                (order, name, started.elapsed().as_millis() as u64, result)
            });
        }

        let mut batches = Vec::new();
        let mut statuses = Vec::new();
        let mut success_count = 0;
        while let Some((order, name, elapsed_ms, result)) = tasks.next().await {
            match result {
                Ok(items) => {
                    success_count += 1;
                    statuses.push((
                        order,
                        ProviderStatus {
                            provider: name.clone(),
                            ok: true,
                            result_count: items.len(),
                            elapsed_ms,
                            error: None,
                        },
                    ));
                    batches.push(RankedBatch {
                        provider_order: order,
                        provider: name,
                        results: items,
                    });
                }
                Err(error) => statuses.push((
                    order,
                    ProviderStatus {
                        provider: name,
                        ok: false,
                        result_count: 0,
                        elapsed_ms,
                        error: Some(error.to_string()),
                    },
                )),
            }
        }
        statuses.sort_by_key(|(order, _)| *order);
        batches.sort_by_key(|batch| batch.provider_order);
        let statuses = statuses
            .into_iter()
            .map(|(_, status)| status)
            .collect::<Vec<_>>();
        if success_count == 0 {
            return Err(CyberSearchError::AllProvidersFailed(status_summary(
                &statuses,
            )));
        }
        Ok((cyber_fuse(batches, "cyber_fusion_v1"), statuses))
    }

    async fn fallback_search(
        &self,
        names: &[String],
        request: &ProviderSearchRequest,
    ) -> Result<(FusionOutput, Vec<ProviderStatus>)> {
        let mut statuses = Vec::new();
        let mut any_success = false;
        for name in names {
            let provider = self.providers.get(name).expect("selected provider exists");
            let started = Instant::now();
            match provider.search(request).await {
                Ok(items) => {
                    any_success = true;
                    statuses.push(ProviderStatus {
                        provider: name.clone(),
                        ok: true,
                        result_count: items.len(),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        error: None,
                    });
                    if !items.is_empty() {
                        return Ok((
                            cyber_fuse(
                                vec![RankedBatch {
                                    provider_order: statuses.len() - 1,
                                    provider: name.clone(),
                                    results: items,
                                }],
                                "first_healthy_v1",
                            ),
                            statuses,
                        ));
                    }
                }
                Err(error) => statuses.push(ProviderStatus {
                    provider: name.clone(),
                    ok: false,
                    result_count: 0,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    error: Some(error.to_string()),
                }),
            }
        }
        if any_success {
            Ok((cyber_fuse(Vec::new(), "first_healthy_v1"), statuses))
        } else {
            Err(CyberSearchError::AllProvidersFailed(status_summary(
                &statuses,
            )))
        }
    }
}

fn normalize_domains(domains: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    domains
        .into_iter()
        .filter_map(|domain| {
            let normalized = domain
                .trim()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .to_ascii_lowercase();
            (!normalized.is_empty() && seen.insert(normalized.clone())).then_some(normalized)
        })
        .collect()
}

fn status_summary(statuses: &[ProviderStatus]) -> String {
    statuses
        .iter()
        .map(|status| {
            format!(
                "{}: {}",
                status.provider,
                status.error.as_deref().unwrap_or("无结果")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_provider_names_constant_is_complete() {
        assert_eq!(crate::config::ALL_PROVIDERS.len(), 7);
    }
}

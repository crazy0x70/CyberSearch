use std::{cmp::Ordering, collections::HashMap};

use crate::{SearchResult, model::FusionDiagnostics, providers::normalize_url};

const RECIPROCAL_RANK_OFFSET: f64 = 60.0;
const CONSENSUS_WEIGHT: f64 = 0.15;

pub(crate) struct RankedBatch {
    pub provider_order: usize,
    pub provider: String,
    pub results: Vec<SearchResult>,
}

pub(crate) struct FusionOutput {
    pub results: Vec<SearchResult>,
    pub diagnostics: FusionDiagnostics,
}

struct EvidenceCluster {
    representative: SearchResult,
    reciprocal_score: f64,
    best_rank: usize,
    first_provider_order: usize,
}

/// CyberFusion v1 combines reciprocal rank with an explicit consensus boost.
/// RRF keeps differently calibrated providers comparable; the boost makes
/// independent agreement visible rather than treating it as an incidental sum.
pub(crate) fn cyber_fuse(batches: Vec<RankedBatch>, pipeline: &str) -> FusionOutput {
    let received_candidates = batches.iter().map(|batch| batch.results.len()).sum();
    let mut accepted_candidates = 0;
    let mut clusters: HashMap<String, EvidenceCluster> = HashMap::new();

    for batch in batches {
        for (zero_based_rank, mut candidate) in batch.results.into_iter().enumerate() {
            let Some(canonical_url) = normalize_url(&candidate.url) else {
                continue;
            };
            accepted_candidates += 1;
            let rank = zero_based_rank + 1;
            candidate.url = canonical_url.clone();
            candidate.providers = vec![batch.provider.clone()];
            let reciprocal = 1.0 / (RECIPROCAL_RANK_OFFSET + rank as f64);

            match clusters.get_mut(&canonical_url) {
                Some(cluster) => {
                    cluster.reciprocal_score += reciprocal;
                    cluster.best_rank = cluster.best_rank.min(rank);
                    if !cluster.representative.providers.contains(&batch.provider) {
                        cluster
                            .representative
                            .providers
                            .push(batch.provider.clone());
                    }
                    keep_richer_metadata(&mut cluster.representative, candidate);
                }
                None => {
                    clusters.insert(
                        canonical_url,
                        EvidenceCluster {
                            representative: candidate,
                            reciprocal_score: reciprocal,
                            best_rank: rank,
                            first_provider_order: batch.provider_order,
                        },
                    );
                }
            }
        }
    }

    let unique_results = clusters.len();
    let consensus_results = clusters
        .values()
        .filter(|cluster| cluster.representative.providers.len() >= 2)
        .count();
    let mut ranked = clusters.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        cyber_score(right)
            .partial_cmp(&cyber_score(left))
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.best_rank.cmp(&right.best_rank))
            .then_with(|| left.first_provider_order.cmp(&right.first_provider_order))
            .then_with(|| left.representative.url.cmp(&right.representative.url))
    });
    let results = ranked
        .into_iter()
        .map(|mut cluster| {
            cluster.representative.score = Some(cyber_score(&cluster));
            cluster.representative
        })
        .collect();

    FusionOutput {
        results,
        diagnostics: FusionDiagnostics {
            pipeline: pipeline.into(),
            received_candidates,
            accepted_candidates,
            unique_results,
            collapsed_duplicates: accepted_candidates.saturating_sub(unique_results),
            consensus_results,
            successful_providers: 0,
            failed_providers: 0,
        },
    }
}

fn cyber_score(cluster: &EvidenceCluster) -> f64 {
    let independent_support = cluster.representative.providers.len().saturating_sub(1) as f64;
    cluster.reciprocal_score * (1.0 + CONSENSUS_WEIGHT * independent_support)
}

fn keep_richer_metadata(target: &mut SearchResult, candidate: SearchResult) {
    if candidate.snippet.as_ref().map_or(0, String::len)
        > target.snippet.as_ref().map_or(0, String::len)
    {
        target.snippet = candidate.snippet;
    }
    if target.published_at.is_none() {
        target.published_at = candidate.published_at;
    }
    if target.title.trim().is_empty() && !candidate.title.trim().is_empty() {
        target.title = candidate.title;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consensus_outweighs_a_single_provider_and_reports_deduplication() {
        let output = cyber_fuse(
            vec![
                RankedBatch {
                    provider_order: 0,
                    provider: "alpha".into(),
                    results: vec![
                        SearchResult::new(
                            "A",
                            "https://example.com/a?utm_source=alpha",
                            Some("short".into()),
                            None,
                            None,
                            "alpha",
                        ),
                        SearchResult::new("B", "https://example.com/b", None, None, None, "alpha"),
                    ],
                },
                RankedBatch {
                    provider_order: 1,
                    provider: "beta".into(),
                    results: vec![SearchResult::new(
                        "A2",
                        "https://example.com/a",
                        Some("a much longer snippet".into()),
                        Some("2026-01-01".into()),
                        None,
                        "beta",
                    )],
                },
            ],
            "cyber_fusion_v1",
        );
        assert_eq!(output.results[0].url, "https://example.com/a");
        assert_eq!(output.results[0].providers, ["alpha", "beta"]);
        assert_eq!(
            output.results[0].snippet.as_deref(),
            Some("a much longer snippet")
        );
        assert_eq!(output.diagnostics.received_candidates, 3);
        assert_eq!(output.diagnostics.unique_results, 2);
        assert_eq!(output.diagnostics.collapsed_duplicates, 1);
        assert_eq!(output.diagnostics.consensus_results, 1);
    }
}

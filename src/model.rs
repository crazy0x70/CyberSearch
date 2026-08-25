use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Parallel,
    Fallback,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SearchInput {
    /// 搜索关键词或自然语言问题。
    pub query: String,
    /// 最终返回结果数，默认 10，受 CYBERSEARCH_MAX_LIMIT 限制。
    #[serde(default)]
    pub max_results: Option<usize>,
    /// 指定供应商名称；省略时使用全部已配置供应商。
    #[serde(default)]
    pub providers: Option<Vec<String>>,
    /// parallel 使用 CyberFusion v1；fallback 按顺序使用首个成功供应商。
    #[serde(default)]
    pub mode: Option<SearchMode>,
    /// 仅返回这些域名的结果。供应商不支持原生过滤时会在本地过滤。
    #[serde(default)]
    pub include_domains: Vec<String>,
    /// 排除这些域名的结果。
    #[serde(default)]
    pub exclude_domains: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub providers: Vec<String>,
}

impl SearchResult {
    pub fn new(
        title: impl Into<String>,
        url: impl Into<String>,
        snippet: Option<String>,
        published_at: Option<String>,
        score: Option<f64>,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            snippet,
            published_at,
            score,
            providers: vec![provider.into()],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ProviderStatus {
    pub provider: String,
    pub ok: bool,
    pub result_count: usize,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AggregateSearchResponse {
    pub query: String,
    pub mode: SearchMode,
    pub results: Vec<SearchResult>,
    pub providers: Vec<ProviderStatus>,
    pub fusion: FusionDiagnostics,
}

/// CyberSearch 对本次候选集的处理摘要。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FusionDiagnostics {
    /// 使用的融合管线版本。
    pub pipeline: String,
    /// 上游供应商合计返回的候选条目数。
    pub received_candidates: usize,
    /// URL 合法并进入融合阶段的条目数。
    pub accepted_candidates: usize,
    /// 规范化 URL 去重后的条目数。
    pub unique_results: usize,
    /// 因 URL 相同而折叠的重复条目数。
    pub collapsed_duplicates: usize,
    /// 至少被两个供应商同时命中的结果数。
    pub consensus_results: usize,
    pub successful_providers: usize,
    pub failed_providers: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProviderInfo {
    pub name: String,
    pub enabled: bool,
    pub requires_api_key: bool,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProviderSearchRequest {
    pub query: String,
    pub limit: usize,
    pub include_domains: Vec<String>,
    pub exclude_domains: Vec<String>,
}

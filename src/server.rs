use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_handler, tool_router,
};

use crate::{
    Config, CyberRouter, SearchInput,
    error::Result as CyberResult,
    providers::{build_providers, provider_info},
};

#[derive(Clone)]
pub struct CyberSearchServer {
    router: Arc<CyberRouter>,
    providers: Arc<Vec<crate::ProviderInfo>>,
}

impl CyberSearchServer {
    pub fn new(config: Config) -> CyberResult<Self> {
        let providers = build_providers(&config)?;
        let info = provider_info(&config);
        Ok(Self {
            router: Arc::new(CyberRouter::new(&config, providers)),
            providers: Arc::new(info),
        })
    }

    fn json_result<T: serde::Serialize>(
        value: &T,
    ) -> std::result::Result<CallToolResult, McpError> {
        let text = serde_json::to_string_pretty(value)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }
}

#[tool_router]
impl CyberSearchServer {
    #[tool(
        description = "聚合多个联网搜索供应商。parallel 模式使用 CyberFusion v1 执行共识增强排序与去重；fallback 模式按配置顺序选择首个健康供应商。"
    )]
    async fn web_search(
        &self,
        Parameters(input): Parameters<SearchInput>,
    ) -> std::result::Result<CallToolResult, McpError> {
        match self.router.search(input).await {
            Ok(response) => Self::json_result(&response),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(
                error.to_string(),
            )])),
        }
    }

    #[tool(
        description = "列出 CyberSearch 支持的搜索供应商、启用状态和 API 基础地址；不会输出 API key。"
    )]
    fn list_providers(&self) -> std::result::Result<CallToolResult, McpError> {
        Self::json_result(self.providers.as_ref())
    }

    #[tool(
        description = "返回脱敏后的 CyberSearch 配置诊断，确认哪些供应商已启用。不会发起计费搜索请求。"
    )]
    fn doctor(&self) -> std::result::Result<CallToolResult, McpError> {
        let enabled = self.providers.iter().filter(|item| item.enabled).count();
        Self::json_result(&serde_json::json!({
            "service": "cybersearch",
            "version": env!("CARGO_PKG_VERSION"),
            "enabled_provider_count": enabled,
            "providers": self.providers.as_ref(),
        }))
    }
}

#[tool_handler(
    name = "cybersearch",
    version = "0.0.1",
    instructions = "聚合 Tavily、Exa、Firecrawl、TinyFish、Grok、Gemini 与 DuckDuckGo 的联网搜索 MCP 路由。"
)]
impl ServerHandler for CyberSearchServer {}

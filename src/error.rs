use thiserror::Error;

pub type Result<T> = std::result::Result<T, CyberSearchError>;

#[derive(Debug, Error)]
pub enum CyberSearchError {
    #[error("配置错误: {0}")]
    Config(String),
    #[error("未知搜索供应商: {0}")]
    UnknownProvider(String),
    #[error("供应商 {provider} 请求失败: {message}")]
    Provider { provider: String, message: String },
    #[error("没有可用的搜索供应商")]
    NoProviders,
    #[error("所有搜索供应商均失败: {0}")]
    AllProvidersFailed(String),
    #[error("序列化失败: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl CyberSearchError {
    pub(crate) fn provider(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.into(),
        }
    }
}

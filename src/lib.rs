mod aggregator;
mod config;
mod error;
mod fusion;
mod model;
mod providers;
mod server;

pub use aggregator::CyberRouter;
/// Backward-compatible name for integrations created before CyberRouter.
pub type SearchAggregator = CyberRouter;
pub use config::{Config, ProviderConfig};
pub use error::{CyberSearchError, Result};
pub use model::{
    AggregateSearchResponse, FusionDiagnostics, ProviderInfo, ProviderSearchOutput, ProviderStatus,
    SearchAudit, SearchInput, SearchMode, SearchResult,
};
pub use providers::{SearchProvider, build_providers};
pub use server::CyberSearchServer;

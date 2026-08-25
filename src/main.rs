use anyhow::Context;
use cybersearch::{Config, CyberSearchServer};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransportMode {
    Stdio,
    #[cfg(feature = "http")]
    Http,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "cybersearch=info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let mode = transport_mode()?;
    let config = Config::from_env().context("加载 CyberSearch 配置失败")?;

    match mode {
        TransportMode::Stdio => run_stdio(config).await,
        #[cfg(feature = "http")]
        TransportMode::Http => run_http(config).await,
    }
}

fn transport_mode() -> anyhow::Result<TransportMode> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => Ok(TransportMode::Stdio),
        #[cfg(feature = "http")]
        Some("--http") if args.next().is_none() => Ok(TransportMode::Http),
        #[cfg(not(feature = "http"))]
        Some("--http") => anyhow::bail!(
            "当前二进制未包含 HTTP transport；请使用 HTTP 预编译包或执行 cargo build --release --features http"
        ),
        Some(argument) => anyhow::bail!("未知参数：{argument}；默认使用 stdio，可选参数为 --http"),
    }
}

async fn run_stdio(config: Config) -> anyhow::Result<()> {
    let server = CyberSearchServer::new(config).context("初始化 CyberSearch MCP 服务失败")?;
    let service = server
        .serve(stdio())
        .await
        .context("启动 MCP stdio 传输失败")?;
    service.waiting().await.context("MCP 服务异常退出")?;
    Ok(())
}

#[cfg(feature = "http")]
async fn run_http(config: Config) -> anyhow::Result<()> {
    use std::net::SocketAddr;

    use axum::{Json, Router, routing::get};
    use rmcp::transport::{
        StreamableHttpServerConfig,
        streamable_http_server::{
            session::local::LocalSessionManager, tower::StreamableHttpService,
        },
    };

    let bind =
        std::env::var("CYBERSEARCH_HTTP_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let address = bind
        .parse::<SocketAddr>()
        .with_context(|| format!("CYBERSEARCH_HTTP_BIND 不是有效地址：{bind}"))?;

    let server = CyberSearchServer::new(config).context("初始化 CyberSearch MCP 服务失败")?;
    let mcp_service: StreamableHttpService<CyberSearchServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

    let app = Router::new()
        .route(
            "/health",
            get(|| async {
                Json(serde_json::json!({
                    "service": "cybersearch",
                    "version": env!("CARGO_PKG_VERSION"),
                    "transport": "streamable-http",
                    "status": "ok"
                }))
            }),
        )
        .nest_service("/mcp", mcp_service);

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("绑定 HTTP 地址失败：{address}"))?;
    tracing::info!(%address, endpoint = "/mcp", "CyberSearch Streamable HTTP 已启动");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "监听退出信号失败");
            }
        })
        .await
        .context("CyberSearch Streamable HTTP 服务异常退出")?;
    Ok(())
}

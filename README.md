<div align="center">
  <h1>CyberSearch</h1>
  <strong>高性能 Rust 聚合联网搜索 MCP Server</strong><br>
  多源并发路由 · CyberFusion 跨源共识重排 · 毫秒级故障转移 · 零配置即开即用
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/cybersearch-mcp"><img src="https://img.shields.io/npm/v/cybersearch-mcp?color=crimson&label=npm%20package" alt="npm version"></a>
  <a href="https://github.com/crazy0x70/CyberSearch/releases"><img src="https://img.shields.io/github/v/release/crazy0x70/CyberSearch?color=blue" alt="Latest Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-green.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/Rust-1.88%2B-orange.svg" alt="Rust 1.88+">
  <img src="https://img.shields.io/badge/MCP-Standard%20Compatible-purple.svg" alt="MCP Compatible">
</div>

---

`CyberSearch` 是一款专为 AI Agent 和大语言模型打造的高性能、低延迟聚合联网搜索 MCP（Model Context Protocol）服务器。它将 **Tavily、Exa、Firecrawl、TinyFish、Grok (xAI)、Gemini (Google Search Grounding)** 以及 **DuckDuckGo** 汇聚于统一接口，搭载专有的 **CyberFusion v1** 融合排序算法，提供多源互证、去重提纯的结构化搜索结果。

---

## ✨ 核心特性

- 🌐 **7 大主流搜索源融合**：聚合 Tavily (RAG 优化)、Exa (语义搜索与摘要)、Firecrawl (深度抓取)、TinyFish (JS 渲染)、Grok (实时信息)、Gemini (Google Search 接地) 与 DuckDuckGo (免 Key 开箱即用)。
- 🧠 **CyberFusion v1 融合算法**：采用 Reciprocal Rank Fusion (RRF) 结合跨供应商独立共识增强（Consensus Boost），智能折叠重复候选，保留最长摘要并动态提权高信度证据。
- ⚡ **双模调度策略**：
  - `parallel`（默认）：全源并发请求，最高召回率与多源交叉互证，单上游超时或故障不影响其他结果。
  - `fallback`：链式顺序探查，首个命中结果即刻返回，最大化节省 API 额度与 Tokens。
- 🧹 **URL 智能规范化**：自动剥离跟踪参数（`utm_*`、`fbclid` 等）、锚点碎片与重定向包装，跨源精准折叠重复 URL。
- 🎯 **全局域名过滤**：一视同仁支持 `include_domains`（白名单）与 `exclude_domains`（黑名单）子域递归匹配。
- 🪶 **开箱即用 / 零编译分发**：预编译全平台原生二进制（Darwin / Linux / Windows, x64 / ARM64），通过 `npm install -g` 一键下发。
- 🔒 **密钥隔离与安全**：API 密钥仅进入对应上游请求头，日志与响应完全脱敏；内置 `doctor` 零额度自检工具。

---

## 🛠️ MCP 工具矩阵

| 工具 | 说明 | 适用场景 |
|---|---|---|
| `web_search` | **核心搜索工具**。统一多源并发/故障转移搜索，输出标准化、共识重排后的结果。 | Agent 联网查证、最新资讯获取、技术文档检索 |
| `list_providers` | 查看当前所有 Provider 的可用状态、Base URL 和生效模型（不泄露密钥）。 | 动态感知当前 MCP 可用的上游引擎能力 |
| `doctor` | 运行快速自检，输出当前配置摘要、版本与脱敏环境报告（**零额度消耗**）。 | 部署排错、配置验证、连通性检查 |

---

## ⚡ 快速开始

```bash
npm install -g cybersearch-mcp
```

> 所有 Provider 均为**可选**配置。未提供 Key 的商业 Provider 会自动跳过；**DuckDuckGo 默认开启且无需任何 Key**，即便零配置也能立即工作。

---

## 💻 MCP Client 配置指南

将 `cybersearch` 注册到您的 MCP 客户端中。根据您持有的 API Key 填入环境变量（不需要的行直接留空或删除即可）：

### 1. Claude Desktop / Cursor / Windsurf

在配置文件的 `mcpServers` 节点下添加：

```json
{
  "mcpServers": {
    "cybersearch": {
      "command": "cybersearch",
      "args": [],
      "env": {
        "CYBERSEARCH_MODE": "parallel",
        "TAVILY_API_KEY": "tvly-...",
        "EXA_API_KEY": "...",
        "GROK_API_KEY": "xai-...",
        "GEMINI_API_KEY": "AIza...",
        "TINYFISH_API_KEY": "...",
        "FIRECRAWL_API_KEY": "fc-..."
      }
    }
  }
}
```

### 2. Codex / 兼容 TOML 配置的客户端

```toml
[mcp_servers.cybersearch]
type = "stdio"
command = "cybersearch"

[mcp_servers.cybersearch.env]
CYBERSEARCH_MODE = "parallel"
TAVILY_API_KEY = "tvly-..."
EXA_API_KEY = "..."
GROK_API_KEY = "xai-..."
GROK_MODEL = "grok-4.6"
GEMINI_API_KEY = "AIza..."
GEMINI_MODEL = "gemini-3.7-flash"
TINYFISH_API_KEY = "..."
FIRECRAWL_API_KEY = "fc-..."
```

---

## 🔍 `web_search` 参数全览

向 Agent 发送搜索请求时的参数结构：

```json
{
  "query": "Rust MCP SDK latest release",
  "max_results": 10,
  "providers": ["tavily", "exa", "gemini"],
  "mode": "parallel",
  "include_domains": ["github.com", "modelcontextprotocol.io"],
  "exclude_domains": ["csdn.net"]
}
```

### 输入参数说明

| 字段 | 类型 | 必填 | 默认值 | 详细说明 |
|---|---|---|---|---|
| `query` | `string` | **是** | — | 搜索关键词或自然语言提问 |
| `max_results` | `number` | 否 | `10` | 最终返回的最大条目数量（受 `CYBERSEARCH_MAX_LIMIT` 限制） |
| `providers` | `string[]` | 否 | 全部可用源 | 本次调用临时指定的引擎列表（例如 `["tavily", "duckduckgo"]`） |
| `mode` | `string` | 否 | `parallel` | 路由调度策略：`parallel`（全源并发融合）或 `fallback`（链式顺序探查） |
| `include_domains` | `string[]` | 否 | `[]` | 仅保留指定域名（包含子域名）的结果 |
| `exclude_domains` | `string[]` | 否 | `[]` | 严格剔除指定域名（包含子域名）的结果 |

### 返回结构解析

```json
{
  "results": [
    {
      "title": "modelcontextprotocol/rust-sdk - GitHub",
      "url": "https://github.com/modelcontextprotocol/rust-sdk",
      "snippet": "Official Rust SDK for Model Context Protocol...",
      "score": 0.0384,
      "providers": ["tavily", "exa", "duckduckgo"],
      "published_date": "2025-02-20"
    }
  ],
  "providers": {
    "tavily": { "status": "success", "count": 5, "latency_ms": 320 },
    "exa": { "status": "success", "count": 4, "latency_ms": 410 }
  },
  "fusion": {
    "raw_candidates": 15,
    "unique_urls": 8,
    "collapsed_duplicates": 7,
    "consensus_count": 3
  }
}
```

---

## 🧠 CyberFusion v1 融合算法

为了消除不同搜索引擎各自私有评分标准的度量差异，CyberSearch 使用了 **CyberFusion v1** 排序机制：

```text
               ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
               │   Tavily    │   │     Exa     │   │ DuckDuckGo  │  ... (All Enabled)
               └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
                      │                 │                 │
                      ▼                 ▼                 ▼
          ┌─────────────────────────────────────────────────────────┐
          │       URL Canonicalization & Tracking Stripping         │
          └───────────────────────────┬─────────────────────────────┘
                                      ▼
          ┌─────────────────────────────────────────────────────────┐
          │             Reciprocal Rank Fusion (RRF)                │
          │             reciprocal_score = Σ 1 / (60 + rank)        │
          └───────────────────────────┬─────────────────────────────┘
                                      ▼
          ┌─────────────────────────────────────────────────────────┐
          │           Multi-Provider Consensus Boost                │
          │    cyber_score = reciprocal_score × (1 + 0.15 × support)│
          └───────────────────────────┬─────────────────────────────┘
                                      ▼
          ┌─────────────────────────────────────────────────────────┐
          │          Snippet Selection & Top-K Ranked Output        │
          └─────────────────────────────────────────────────────────┘
```

1. **倒数排名加权 (RRF)**：对每个 Provider 内部命中的条目按位次计算 `1 / (60 + rank)`。
2. **证据簇合并 (Evidence Cluster)**：将相同 Canonical URL 的结果聚合，保留最详细摘要与发布日期。
3. **共识增强提权 (Consensus Boost)**：多引擎同时命中的结果视为高信度证据，按照 `(1 + 0.15 × (命中供应商数 - 1))` 进行动态加权。
4. **全透明诊断**：响应中保留 `fusion` 遥测数据，方便 Agent 评估信息来源权威度。

---

## ⚙️ 完整环境变量配置表

CyberSearch 从进程环境变量读取配置。

### 1. 搜索供应商配置

| 环境变量 | 默认值 | 必需 | 说明 |
|---|---|---|---|
| `TAVILY_API_KEY` | — | 否 | Tavily Search API Key |
| `TAVILY_BASE_URL` | `https://api.tavily.com` | 否 | Tavily API 代理/自建网关 |
| `EXA_API_KEY` | — | 否 | Exa 语义搜索 API Key |
| `EXA_BASE_URL` | `https://api.exa.ai` | 否 | Exa API 代理/自建网关 |
| `GROK_API_KEY` | — | 否 | xAI / Grok API Key |
| `GROK_BASE_URL` | `https://api.x.ai` | 否 | Grok Responses API 网关 |
| `GROK_MODEL` | `grok-4.6` | 否 | 使用的 Grok 模型名称 |
| `GEMINI_API_KEY` | — | 否 | Google Gemini API Key |
| `GEMINI_BASE_URL` | `https://generativelanguage.googleapis.com` | 否 | Gemini API 网关 |
| `GEMINI_MODEL` | `gemini-3.7-flash` | 否 | 使用的 Gemini 模型名称 |
| `FIRECRAWL_API_KEY` | — | 否 | Firecrawl Search v2 API Key |
| `FIRECRAWL_BASE_URL` | `https://api.firecrawl.dev` | 否 | Firecrawl 实例/网关 |
| `TINYFISH_API_KEY` | — | 否 | TinyFish Search API Key |
| `TINYFISH_BASE_URL` | `https://api.search.tinyfish.ai` | 否 | TinyFish 网关 |
| `DUCKDUCKGO_BASE_URL` | `https://html.duckduckgo.com` | 否 | DuckDuckGo HTML 入口（免 Key） |

### 2. 全局行为与性能配置

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `CYBERSEARCH_MODE` | `parallel` | 默认路由模式（`parallel` 并发 / `fallback` 故障转移链） |
| `CYBERSEARCH_PROVIDERS` | `tavily,exa,firecrawl,tinyfish,grok,gemini,duckduckgo` | 默认启用的 Provider 优先级顺序 |
| `CYBERSEARCH_TIMEOUT_SECONDS` | `30` | 所有上游单次 HTTP 请求超时时间（秒） |
| `CYBERSEARCH_DEFAULT_LIMIT` | `10` | 未指定 `max_results` 时的默认返回数量 |
| `CYBERSEARCH_MAX_LIMIT` | `30` | 允许请求的最大返回数量上限 |
| `CYBERSEARCH_USER_AGENT` | `CyberSearch/0.0.1` | 上游 HTTP 请求携带的 User-Agent |
| `CYBERSEARCH_HTTP_BIND` | `127.0.0.1:8080` | 启用 HTTP 特性时的监听地址 |
| `RUST_LOG` | `cybersearch=info` | 日志输出级别过滤 |

---

## 🌐 远程部署 (Streamable HTTP)

CyberSearch 支持构建为远程 **Streamable HTTP MCP Server**，方便移动端、多设备或集群统一调用。

### 1. 启动 HTTP 服务

使用支持 HTTP 特性的二进制（或本地源码编译）：

```bash
cargo build --release --features http
CYBERSEARCH_HTTP_BIND=0.0.0.0:8080 ./target/release/cybersearch --http
```

- **MCP 访问端点**：`http://<your-ip>:8080/mcp`
- **健康检查探针**：`http://<your-ip>:8080/health`

### 2. 反向代理与安全建议

在生产环境中，建议将 `cybersearch` 置于反向代理（如 Caddy、Nginx 或 Cloudflare Tunnel）之后，配置 HTTPS 终结与安全访问控制：

```caddy
mcp.yourdomain.com {
    reverse_proxy localhost:8080
}
```

客户端接入（以 Claude CLI 为例）：

```bash
claude mcp add --transport http cybersearch https://mcp.yourdomain.com/mcp
```

---

## 🏗️ 源码构建与本地开发

环境要求：**Rust 1.88+**

```bash
# 克隆仓库
git clone https://github.com/your-repo/CyberSearch.git
cd CyberSearch

# 编译标准 stdio 二进制
cargo build --release

# 编译带 HTTP 服务的二进制
cargo build --release --features http
```

### 本地测试与代码质量验证

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

---

## 📂 项目架构

```text
CyberSearch/
├── src/
│   ├── main.rs               # stdio / Streamable HTTP 启动分发
│   ├── server.rs             # MCP 协议握手与 Tool Handler 实现
│   ├── aggregator.rs         # 并发调度器与 Fallback 状态机
│   ├── fusion.rs             # CyberFusion v1 融合与打分算法
│   ├── model.rs              # MCP 入参、出参及诊断遥测结构
│   ├── config.rs             # 环境变量提取与 Provider 注册表
│   └── providers/            # 7 大搜索源适配器
│       ├── tavily.rs
│       ├── exa.rs
│       ├── grok.rs
│       ├── gemini.rs
│       ├── duckduckgo.rs
│       ├── tinyfish.rs
│       └── firecrawl.rs
├── npm/                      # npm 全平台原生二进制下发工程
├── scripts/                  # 多架构版本同步与打包脚本
├── tests/                    # 集成与契约测试
└── Cargo.toml
```

---

## 📄 开源许可证

本项目采用 [MIT License](LICENSE) 许可证开源。

# cybersearch-mcp

CyberSearch 的预编译 npm 分发包。安装过程不编译 Rust。

```bash
npm install -g cybersearch-mcp
```

安装后，在 MCP Client 中把 `command` 设置为：

```text
cybersearch
```

当前系统对应的原生平台包会通过 `optionalDependencies` 自动安装；不编译 Rust，也不会下载其他平台的二进制。

API key、base URL 和 model 通过 MCP Client 的 `env` 配置传入。完整示例见 CyberSearch 项目 README。

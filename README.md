# ThetaGPToolkit

ThetaGP 的**主机侧唯一实现**：协议客户端、设备工具、契约测试、前端与 MCP server。

- 一个 Cargo 工程，一个 Rust 核心，三个消费者共用它：**前端**、**CLI**、**MCP**。
- 依赖 `ThetaGP.PB` 提供协议（钉同一个 tag），与固件**互不依赖**：
  固件不需要本工程即可构建；本工程不读固件仓或它的构建目录。

范围与设计见 [`docs/scope.md`](docs/scope.md)。

## 现在有什么

| crate | 状态 |
|---|---|
| `thetagp-wire` | 帧、校验和、varint 前缀、半帧超时、分片重组 |
| `thetagp-proto` | 待建（从 `ThetaGP.PB` 生成类型）|
| `thetagp-client` | 待建 |
| `thetagp-cli` / `thetagp-mcp` / `thetagp-app` | 待建 |

```bash
cargo test
```

# AI / MCP Module

AI/MCP 模块提供 Runtime AI、Editor Copilot、Content Generation 和 MCP tool host。它依赖 PropertySystem、diagnostics、schema 和 audit，不进入 Core。

## Runtime AI

Runtime AI intent 只能通过 IntentValidator 进入 Runtime。提交后写入 CommittedAiOutput，save/replay 使用固化 payload。

## Editor Copilot

Copilot 可解释 diagnostics、生成 patch、批量修复 schema、辅助 release report 和创建测试 scenario。未授权时进入 Review Queue；Trusted session 可直写但必须可回滚。

## Content Generation

生成图片、音频、视频、文本或脚本草稿时先创建 AI draft sidecar。Draft 被接受后才进入 AssetRegistry 或 canonical source。

## v1 Gate

AI/MCP v1 同时覆盖创作和运行时：Runtime AI committed output、Editor Copilot、Content Generation、MCP tool host、Trusted session、Review Queue、audit、rollback 和 provider-free replay。接口和检查见 [AI And MCP Runtime Blueprint](../implementation/ai-mcp-runtime.md)。

# AI / MCP Module

AI/MCP 模块提供 Runtime Director、Editor Copilot、Content Generation、MCP tool host、Context Pack 和 Runtime Memory。它依赖 PropertySystem、diagnostics、schema、save/package section 和 audit，不进入 Core。

## Runtime AI

Runtime 通过受限 `McpAiSession` 调模型，不直接持有 OpenAI、Ollama、ComfyUI 或 ONNX Runtime provider。模型输出只能是 typed Intent、MCP tool call 或 typed generated artifact chunk，Intent 和 artifact 通过 validator 后写入 CommittedAiOutput，save/replay 使用固化 payload。

`astra-ai-onnx` 是一方本地 provider 设计。模型、tokenizer、reduced runtime、custom op sidecar 和 Web runtime adapter 都通过 cook/package 成为 ModelBundle，并由 package/VFS section ref 读取；Runtime provider 不能读取 loose file 或绝对路径。

## Runtime Memory

角色记忆按 `Canon`、`Episodic`、`Player` 分权威来源，按 working、short-term、long-term、archive 分层。创作者限制 AI 可读写的 namespace；短期记忆可按策略自动压缩归档，归档结果进入 committed ledger、audit 和 replay。

## Editor Copilot

Copilot 可解释 diagnostics、生成 patch、批量修复 schema、辅助 release report 和创建测试 scenario。未授权时进入 Review Queue；Trusted session 可直写但必须可回滚。OpenAI、Ollama 和 ComfyUI 通过 provider profile 接入，默认由项目显式绑定。

## Content Generation

生成图片、音频、视频、文本或脚本草稿时先创建 AI draft sidecar。Draft 被接受后才进入 AssetRegistry 或 canonical source。Shipping Runtime 中的 ONNX 文本、图像和语音生成不走 draft sidecar；通过 validator 后写入 save extra section，正式 replay 不重跑 provider。

## MCP Context

外部工具默认接收 Context Pack，再通过 `context.read/search` 和 `memory.read/search` 按需读取授权范围。Mutating tool 必须产出 patch、audit event、undo checkpoint 和 rollback policy。命令执行只允许项目声明的 allowlist。

## v1 Gate

AI/MCP v1 同时覆盖创作和运行时：Runtime Director + 角色记忆、provider profile、ONNX ModelBundle、Editor AI Control、Memory Inspector、Content Generation、MCP tool host、Trusted session、Review Queue、audit、rollback、debug trace redaction、player consent 和 provider-free replay。接口和检查见 [AI And MCP Runtime Blueprint](../implementation/ai-mcp-runtime.md)。

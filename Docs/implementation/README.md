# Implementation Blueprint

本目录把产品契约落到接近代码的实现规格。实现者应先读本页，再按 Stage 顺序进入具体规格；后序产品不得绕过这些边界反向改 EngineCore。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [workspace-blueprint.md](workspace-blueprint.md) | Rust workspace、crate、feature、binary、依赖方向 |
| [phase-delivery.md](phase-delivery.md) | Stage 1-6 v1 闭环，以及 Stage 7/8 AstraRPG planned extension 的命令、报告和退出标准 |
| [runtime-api.md](runtime-api.md) | RuntimeWorld lifecycle、Actor/Component、StateMachine、Debug API |
| [target-platform.md](target-platform.md) | Editor/Game/Program Target、project.yaml targets、六平台 SDK 分层验收 |
| [state-machine-action-provider.md](state-machine-action-provider.md) | StateMachine action provider、deterministic context、FFI effect list |
| [provider-plugin-api.md](provider-plugin-api.md) | 插件 descriptor、provider trait、权限、load/unload lifecycle |
| [asset-vfs.md](asset-vfs.md) | VFS mount family、package/local/legacy/overlay source、reader provider 和 release gate |
| [asset-media-pipeline.md](asset-media-pipeline.md) | Asset import/cook/package、Media command、默认 provider 和 graph validation |
| [game-runtime-provider.md](game-runtime-provider.md) | NativeVN、AstraEMU、AstraRPG 同级 gameplay runtime provider 选择层 |
| [astra-rpg-runtime.md](astra-rpg-runtime.md) | AstraRPG provider、RPG core、`rpg.trpg` profile、AI Town 和 CP2020 local-private adapter |
| [astra-grammar-ir.md](astra-grammar-ir.md) | `.astra` pest grammar、AST、IR、source map、formatter、错误恢复 |
| [astra-vn-state-machine.md](astra-vn-state-machine.md) | AstraVN command cursor、wait state、StateMachine action 和演出调度 |
| [runtime-execution.md](runtime-execution.md) | tick 顺序、EventQueue、AwaitToken/Fence、MutationLog、hash/replay |
| [luau-policy.md](luau-policy.md) | Luau host API、sandbox、typed policy、pesde、lock/vendor cache |
| [../modules/astra-vn-presentation-model.md](../modules/astra-vn-presentation-model.md) | Stage/Layer/Camera/Sprite/TextWindow/VideoLayer 和 advanced presentation profile |
| [../modules/astra-vn-standard-commands.md](../modules/astra-vn-standard-commands.md) | AstraVN 标准命令库、schema、IR、Editor metadata 和 release check |
| [../modules/astra-vn-system-ui-profile.md](../modules/astra-vn-system-ui-profile.md) | save/load、config、backlog、gallery、replay、route chart、voice replay 和 localization preview |
| [package-save.md](package-save.md) | serde+postcard section、schema/migrator、package/save/replay 容器、AI ModelBundle package/VFS section |
| [editor-visual-protocol.md](editor-visual-protocol.md) | Graph/Timeline 派生层、Inspector 控件、source round-trip |
| [editor-workflow.md](editor-workflow.md) | UE 级 creator workflow、面板状态、Qt/Rust bridge |
| [editor/README.md](editor/README.md) | AstraEditor 前端设计总索引（cxx-qt、Dock、Graph、Script、AI Copilot；Stage 4 完整设计稿） |
| [editor/shell.md](editor/shell.md) | cxx-qt bridge、Dock 布局、设计系统、PIE Viewport、Inspector、Content Browser、Plugin Manager、i18n、快捷键 |
| [editor/graph.md](editor/graph.md) | Graph Editor（NodeEditor-Qt）、Timeline Editor、FilterGraph/AudioGraph Editor、source round-trip |
| [editor/script-editor.md](editor/script-editor.md) | Script Editor（tree-sitter + ropey）、语法高亮、source map badge、查找/替换、astra-lsp 规划 |
| [editor/ai-copilot.md](editor/ai-copilot.md) | AI Copilot（inline hint + Review Queue）、Trusted session、MCP、AI provider 配置、AI gate |
| [ai-mcp-runtime.md](ai-mcp-runtime.md) | Runtime AI、Editor Copilot、Content Generation、MCP tool policy |
| [ai-provider-profiles.md](ai-provider-profiles.md) | OpenAI、Ollama、ComfyUI、ONNX Runtime provider profile 和第一方插件边界 |
| [runtime-ai-director-memory.md](runtime-ai-director-memory.md) | Runtime Director、角色记忆、Context Pack 和 Intent |
| [mcp-context-tooling.md](mcp-context-tooling.md) | 外部 AI 工具、MCP context、命令白名单和 audit |
| [ai-release-gate.md](ai-release-gate.md) | AI/MCP release check、debug trace、玩家同意和 provider-free replay |
| [platform-host.md](platform-host.md) | 六平台 host trait、capability report、profile gate |
| [astraemu-legacy-runtime-framework.md](astraemu-legacy-runtime-framework.md) | AstraEMU LegacyRuntimeProvider、session、auto probe、Trusted Luau、文本翻译、filter preset 和 release gate |
| [emulator-core-state-machine.md](emulator-core-state-machine.md) | EmulatorCore 复用 RuntimeWorld/StateMachine/VFS 的旧 VM 映射、scheduler 和 family 样板 |
| [astraemu-artemis-core.md](astraemu-artemis-core.md) | Artemis v1 engine-native family plugin、probe、snapshot、report |
| [release-gate-report.md](release-gate-report.md) | machine-readable report、blocking checks、证据格式 |
| [release-gate-checks.md](release-gate-checks.md) | release check id、domain、输入、阻断条件和 evidence |

## 实施规则

- Rust 类型是 schema 真源。实现后由 `serde` + `schemars` 生成 JSON Schema，文档字段名必须跟 Rust 类型一致。
- 每个 Stage 必须产出可运行命令、machine-readable report 和测试证据。
- 全系列 v1 必须覆盖 EngineCore、AstraVN、AstraEditor、AstraPlatform、AstraEMU；AstraEMU v1 family 是 Artemis。AstraRPG 属于 Stage 7 planned extension，Stage 8 再接 Server/Client protocol。
- 玩法类型通过 `ProductRuntimeProvider`/`GameRuntimeProvider` 显式绑定；AstraVN、AstraEMU 和后续 AstraRPG 是同级 provider。
- TRPG 玩法通过 AstraRPG 的 `rpg.trpg` profile 接入，不创建独立顶层 `AstraTRPG` 模块或 provider。
- AstraVN policy 统一使用 Luau。AstraEMU 研究文档中的 Lua/TJS 是 legacy engine 事实，不作为 AstraVN policy 术语。
- Stage 依赖单向流动：Stage N 只能依赖前序稳定契约。确需回改契约时，同步 ADR、migration、测试矩阵和 release gate。

## 验证

```bash
python Tools/check_docs.py
git diff --check
```

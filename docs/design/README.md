# AstraEngine 设计文档

状态：Target Architecture  
定位：高度可定制化、模块化的 2D 引擎。视觉小说是第一垂直模块，同时支持传统 VN、AI 协作和运行时受控 AI 内容。旧 VN 引擎模拟器和旧作现代化是 native runtime production parity 之后的扩展轨。

## 1. 核心定位

AstraEngine 不是 AAA 通用 3D 引擎，也不是单一 VN 播放器。它的目标是提供一套轻量但完整的 2D 叙事与演出引擎基础：

- 通用 2D Core 不绑定 VN、Lua、Live2D、AI Provider 或旧引擎语义。
- Actor/Component 是公开对象模型，状态机是运行时核心抽象。
- EventBus 连接脚本、输入、动画完成、Timeline、AI Intent 和系统事件。
- Presentation.VN / AstraVN 是第一垂直模块，提供 Dialogue、Choice、Character、Background、Timeline 等预定义状态机。
- ScriptRuntimeHost 目标上支持 Astra Native Script、Lua、旧 VN VM 和自定义脚本运行时并存；旧 VN VM 属于后期兼容扩展轨。
- FilterGraph 是风格化演出、后处理和旧游戏现代化的统一管线。
- AI 只输出结构化 Intent，由 Validator、ControlPolicy 和 Director 仲裁后转为事件。
- Legacy VN 支持以模拟器和现代化插件实现，不要求反编译或导入为 Astra 源项目；它依赖稳定 Runtime/Asset/Media/Script API，不能成为 native runtime 达标前置条件。

## 2. 文档地图

| 文档 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 总体分层、核心抽象、Creator Experience Parity、Customization Parity、公共契约和模块边界 |
| [goals.md](goals.md) | UE 级 2D/VN runtime、创作者体验、可定制度的总目标和非目标 |
| [foundation-core-platform-property.md](foundation-core-platform-property.md) | Core 类型、diagnostics、config、serialization、Platform service 和 PropertySystem |
| [actor-component-ecs-hybrid.md](actor-component-ecs-hybrid.md) | Actor/Component、创作者可见模型、Inspector metadata、状态机组件、ControlPolicy 与局部 ECS 使用规则 |
| [runtime-core.md](runtime-core.md) | RuntimeWorld、deterministic tick、RuntimeEvent、Scheduler、StateMachine、Director、Save/Replay 和 Debugger |
| [asset-pipeline.md](asset-pipeline.md) | AssetId、VFS、Importer、Cooker、DDC、Package Manifest、Hot Reload 和 Asset Release Gate |
| [media-runtime.md](media-runtime.md) | Renderer2D、TextLayout、Audio、Timeline、UI、FilterGraph、provider slot 和 media release gate |
| [script-and-presentation.md](script-and-presentation.md) | ScriptRuntimeHost、Native DSL、Lua、Graph/Timeline、PresentationCommand 和 AstraVN 模块 |
| [extension-and-module-system.md](extension-and-module-system.md) | ModuleManager、ServiceRegistry、ExtensionRegistry、C ABI、Plugin Wizard、Provider contract、权限和热重载 |
| [content-and-assets.md](content-and-assets.md) | Text-First 项目源数据、Project Template、Asset Import Preset、AssetId、sidecar、Review Queue 和 FilterProfile |
| [editor-and-pipeline.md](editor-and-pipeline.md) | Project Wizard、Content Browser、Inspector、Graph/Timeline、PIE、Cook/Package、Release Gate 和现代化工作流 |
| [editor-ui-ai-collaboration-prototype.md](editor-ui-ai-collaboration-prototype.md) | Editor UX spec：Docking、Command Palette、Context Menu、Property Details、Review Queue、Editor Copilot MCP 和 Content Generation MCP |
| [ai-collaboration.md](ai-collaboration.md) | Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP、Provider、Audit 和安全边界 |
| [mcp-integration.md](mcp-integration.md) | Agent 能力协议层，区分 Editor MCP Host 与 Runtime MCP Host |
| [tools-release-observability.md](tools-release-observability.md) | `astra` CLI、validation、cook/package、Release Gate、trace、profiling、crash report 和 CI 矩阵 |
| [samples-and-test-matrix.md](samples-and-test-matrix.md) | NativeVN、RuntimeStress、PackageSmoke、ScriptParity、MediaBackend、AIIntentSafety、CreatorWorkflow、CustomizationPlugin、CompatMockExpansion |
| [implementation-coverage.md](implementation-coverage.md) | 设计覆盖矩阵，映射系统、公共契约、TODO、验收证据和非目标 |
| [compatibility-layer.md](compatibility-layer.md) | 旧 VN 模拟器、包读取、VM、API Mapper、调试和现代化覆盖 |
| [roadmap.md](roadmap.md) | MVP 阶段、验收标准和风险 |
| [TODO.md](TODO.md) | 按新架构拆分的任务清单 |
| [glossary.md](glossary.md) | 核心术语 |
| [../adr](../adr) | 当前目标架构决策记录 |

## 2.1 Production Contract 草案

下列文档是 Phase 5+ 的准 API 草案，用于把 target architecture 细化为可实现契约。它们不表示当前代码已经实现 production backend；实现状态仍以 `TODO.md`、测试和 release evidence 为准。

| 文档 | 内容 |
| --- | --- |
| [runtime-production-contract.md](runtime-production-contract.md) | Runtime tick、scheduler、event ordering、Director arbitration、Actor lifecycle 生产契约 |
| [save-replay-production-contract.md](save-replay-production-contract.md) | Save container、section manifest、migration、replay stream、mismatch localization |
| [asset-package-production-contract.md](asset-package-production-contract.md) | Importer、Cooker、DDC、package streaming、hot reload rollback、Asset Release Gate |
| [media-backend-production-contract.md](media-backend-production-contract.md) | Renderer2D、TextLayout、Audio、Timeline、FilterGraph provider 执行契约 |
| [hardware-media-decode.md](hardware-media-decode.md) | 独立 Image/Audio/Video Decode Provider、硬解 capability、zero-copy/fallback、diagnostics |
| [provider-contracts.md](provider-contracts.md) | Provider descriptor、capability negotiation、permission、hot reload、shutdown、ABI/release gate |
| [editor-runtime-creator-contract.md](editor-runtime-creator-contract.md) | PIE/debug/Inspector/Creator workflow 与 Runtime 连接契约 |
| [ai-mcp-safety-contract.md](ai-mcp-safety-contract.md) | Runtime AI intent、Editor Copilot、Content Generation、review/audit/save replay |
| [legacy-compatibility-contract.md](legacy-compatibility-contract.md) | CompatRuntimeProvider、LegacyPackageReader、SaveExtensionStateProvider 边界 |
| [release-gate-observability-contract.md](release-gate-observability-contract.md) | Release report、diagnostics policy、trace/profiling、crash bundle |

## 3. 设计不变量

- `Core` 不包含 VN 剧情、Live2D、AI 模型、旧引擎 VM 或编辑器语义。
- Runtime 不依赖 Editor。
- Editor 必须提供 UE 级创作者工作流：模板、Content Browser、Inspector、Graph/Timeline、PIE、Debug、Cook/Package。
- 引擎必须提供 UE 级可定制度：plugin、EngineModuleSlot、Editor panel、asset importer、provider、AI/MCP tool。
- 创作者体验的正式状态流是 `Template -> Project -> Content -> PIE -> Package`。
- AI 和工具生成内容的正式状态流是 `Draft -> Review -> Accepted -> Canonical Source -> Cooked`。
- Copilot 修改的正式状态流是 `Suggestion -> Patch Proposal -> Review -> Apply`。
- Runtime AI 的正式状态流是 `Runtime Feedback -> AIIntent -> Validate -> Commit -> Save/Replay`。
- VN、AI、Legacy Compat、Live2D、Spine、Filter Pack 都是模块或插件；Legacy Compat 是后期扩展轨。
- 创作者 DSL 不直接调用渲染或音频底层 API，而是转成事件和 Presentation Command。
- AI 不直接跳剧情、不直接改核心变量、不直接调用底层 API。
- 旧 VN 模拟器可以拥有自己的 VM 状态，但输出必须通过 Astra 的 Presentation、Asset、Audio、Input、Save 和 FilterGraph 边界。
- 存档保存 Actor、StateMachine、Blackboard、Script Runtime、AI committed output、Filter、Timeline 和 legacy extension state 的确定性快照。

## 4. 推荐阅读顺序

`README` -> `goals` -> `architecture` -> `implementation-coverage` -> `foundation-core-platform-property` -> `runtime-core` -> `runtime-production-contract` -> `save-replay-production-contract` -> `actor-component-ecs-hybrid` -> `asset-pipeline` -> `asset-package-production-contract` -> `media-runtime` -> `hardware-media-decode` -> `media-backend-production-contract` -> `provider-contracts` -> `script-and-presentation` -> `extension-and-module-system` -> `content-and-assets` -> `editor-and-pipeline` -> `editor-runtime-creator-contract` -> `ai-collaboration` -> `ai-mcp-safety-contract` -> `mcp-integration` -> `tools-release-observability` -> `release-gate-observability-contract` -> `samples-and-test-matrix` -> `roadmap` -> `compatibility-layer` -> `legacy-compatibility-contract`。

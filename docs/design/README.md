# AstraEngine 设计文档

状态：Target Architecture  
定位：高度可定制化、模块化的 2D 引擎。视觉小说是第一垂直模块，同时支持传统 VN、AI 协作、运行时受控 AI 内容、旧 VN 引擎模拟器和旧作现代化。

## 1. 核心定位

AstraEngine 不是 AAA 通用 3D 引擎，也不是单一 VN 播放器。它的目标是提供一套轻量但完整的 2D 叙事与演出引擎基础：

- 通用 2D Core 不绑定 VN、Lua、Live2D、AI Provider 或旧引擎语义。
- Actor/Component 是公开对象模型，状态机是运行时核心抽象。
- EventBus 连接脚本、输入、动画完成、Timeline、AI Intent 和系统事件。
- Presentation.VN / AstraVN 是第一垂直模块，提供 Dialogue、Choice、Character、Background、Timeline 等预定义状态机。
- ScriptRuntimeHost 支持 Astra Native Script、Lua、旧 VN VM 和自定义脚本运行时并存。
- FilterGraph 是风格化演出、后处理和旧游戏现代化的统一管线。
- AI 只输出结构化 Intent，由 Validator、ControlPolicy 和 Director 仲裁后转为事件。
- Legacy VN 支持以模拟器和现代化插件实现，不要求反编译或导入为 Astra 源项目。

## 2. 文档地图

| 文档 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 总体分层、核心抽象、运行时数据流和模块边界 |
| [actor-component-ecs-hybrid.md](actor-component-ecs-hybrid.md) | Actor/Component、状态机组件、ControlPolicy 与局部 ECS 使用规则 |
| [extension-and-module-system.md](extension-and-module-system.md) | ModuleManager、ServiceRegistry、ExtensionRegistry、C ABI、权限和热重载 |
| [content-and-assets.md](content-and-assets.md) | Text-First 项目源数据、AssetId、sidecar、外部资产引用和 FilterProfile |
| [editor-and-pipeline.md](editor-and-pipeline.md) | 编辑器、创作链路、Cook/Package、Release Gate 和现代化工作流 |
| [ai-collaboration.md](ai-collaboration.md) | Editor AI 协作、Runtime AI Intent、Provider、Audit 和安全边界 |
| [mcp-integration.md](mcp-integration.md) | Agent 能力协议层，区分 Editor MCP 与 Runtime MCP |
| [compatibility-layer.md](compatibility-layer.md) | 旧 VN 模拟器、包读取、VM、API Mapper、调试和现代化覆盖 |
| [roadmap.md](roadmap.md) | MVP 阶段、验收标准和风险 |
| [TODO.md](TODO.md) | 按新架构拆分的任务清单 |
| [glossary.md](glossary.md) | 核心术语 |
| [../adr](../adr) | 当前目标架构决策记录 |
| [../development](../development) | 当前已实现 Phase 1 工程的开发、构建、测试与 ABI 说明 |

## 3. 设计不变量

- `Core` 不包含 VN 剧情、Live2D、AI 模型、旧引擎 VM 或编辑器语义。
- Runtime 不依赖 Editor。
- VN、AI、Legacy Compat、Live2D、Spine、Filter Pack 都是模块或插件。
- 创作者 DSL 不直接调用渲染或音频底层 API，而是转成事件和 Presentation Command。
- AI 不直接跳剧情、不直接改核心变量、不直接调用底层 API。
- 旧 VN 模拟器可以拥有自己的 VM 状态，但输出必须通过 Astra 的 Presentation、Asset、Audio、Input、Save 和 FilterGraph 边界。
- 存档保存 Actor、StateMachine、Blackboard、Script Runtime、AI committed output、Filter、Timeline 和 legacy extension state 的确定性快照。

## 4. 推荐阅读顺序

`README` -> `architecture` -> `actor-component-ecs-hybrid` -> `extension-and-module-system` -> `content-and-assets` -> `ai-collaboration` -> `compatibility-layer` -> `editor-and-pipeline` -> `roadmap`。

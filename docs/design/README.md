# AstraEngine 设计文档

状态：Draft  
整理日期：2026-05-27  
来源：`D:/Downloads/Astraengine_architecture_design.md`

## 1. 文档目的

本目录用于沉淀 AstraEngine 的架构设计。原始设计稿中的项目名为 AstraEngine，当前仓库名为 AstraEngine；在正式命名前，本文档统一使用“AstraEngine”表示同一引擎方向。后续若确定产品命名，应通过 ADR 或设计变更统一替换。

系统定位为：

> Author-Controlled AI-Assisted VN Production System

即创作者主导、AI 辅助、可审查、可回滚、可扩展、可发布的视觉小说工业化制作系统。

## 2. 文档地图

| 文档 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 总体架构、模块边界、分层模型、运行时服务、动态模块系统和 C++ 技术基线 |
| [ai-collaboration.md](ai-collaboration.md) | AI 三类职责、Boundary Manager、Review Queue、Runtime Generation Orchestrator、Provider/Audit 模块和运行时约束 |
| [content-and-assets.md](content-and-assets.md) | Text-First 源数据、资产 sidecar、YAML + JSON Schema、AI/MCP 友好的内容格式 |
| [editor-and-pipeline.md](editor-and-pipeline.md) | 编辑器工作流、项目目录、资产生命周期、Cook/Package、发布模式和质量门禁 |
| [editor-ui-ai-collaboration-prototype.md](editor-ui-ai-collaboration-prototype.md) | Vue 编辑器 UI 原型、游戏编辑器式布局、AI 协作边界、Qt 实现映射 |
| [extension-and-module-system.md](extension-and-module-system.md) | 动态模块优先、ModuleManager、ExtensionRegistry、VN Property System、插件 ABI、权限和打包规则 |
| [mcp-integration.md](mcp-integration.md) | 统一 MCP / Agent 能力层、Editor MCP Host、Runtime MCP Host、resources、tools、prompts、session 和审计模型 |
| [compatibility-layer.md](compatibility-layer.md) | 外部 VN 引擎兼容层、动态模块扩展、Mount-Only、foreign asset 和现代化覆盖 |
| [roadmap.md](roadmap.md) | MVP 路线、阶段目标、验收标准、风险与优先级 |
| [TODO.md](TODO.md) | 按设计拆分的具体任务清单、优先级、依赖顺序和验收标准 |
| [glossary.md](glossary.md) | 核心术语和约定 |
| [../adr](../adr) | 架构决策记录，包括动态模块优先、编辑器 UI、Renderer2D 后端、Runtime ECS、统一 MCP 能力层、Text-First Source Data 和兼容扩展策略 |

建议阅读顺序：`README` -> `architecture` -> `extension-and-module-system` -> `content-and-assets` -> `editor-and-pipeline` -> `ai-collaboration` -> `mcp-integration` -> `compatibility-layer` -> `roadmap`。

## 3. 设计原则摘要

### 3.1 创作者主导

AI 输出默认是建议，不直接覆盖正式内容。所有可进入作品正文、设定、资源或发布包的 AI 输出都必须满足：

- 有来源记录。
- 可审查。
- 可回滚。
- 可追踪到输入上下文和模型信息。
- 可在发布前生成审计报告。

### 3.2 Editor/Game 分离

编辑器可以依赖运行时，运行时不能依赖编辑器。发布后的游戏包只包含 Runtime、Game Module 和 Cooked Content，不包含 Agent Workbench、Review Queue UI、Diff/Patch Viewer 等开发工具。

### 3.3 Runtime Services 优先

系统以 Astra Runtime 和稳定 Runtime Services 为中心。Astra DSL、Story Graph、Compatibility Modules、Runtime MCP Host、Runtime Generation Orchestrator、Editor Preview、Headless Test 都通过同一套 Stage、Dialogue、Choice、Audio、Asset、Input、Save、Localization 服务运行。Runtime Services 内部采用 Bevy 风格 ECS World 组织运行时状态和系统调度，但对外仍暴露稳定服务 facade 和 Astra RuntimeCommand 协议。

### 3.4 ECS 内部化

ECS 用于提升运行时状态组合能力、数据局部性和系统调度清晰度，但不改变创作者主导、确定性发布和可审查 AI 的目标。EnTT 是 C++ 内部实现依赖，不向 VN DSL、Editor、Compatibility 插件或 AI 插件公开。

### 3.5 动态模块优先

AstraEngine 只专注视觉小说，但在 VN 领域内追求 UE 级可定制化。LLM/Image/TTS Provider、Runtime Generation、Agent Audit、Live2D、Spine、外部引擎兼容模块、资产验证器、Cook Processor、Editor 面板、MCP host/tool 和 Story Graph 节点都应通过动态模块扩展。源码级模块只用于引擎核心、实验性底层能力或尚未稳定 ABI 的内部代码。

### 3.6 VN Property System

借鉴 UE 的工具链、模块组织和编辑器扩展思想，但不复制 UObject/UHT/U++ 式重型元对象体系。Astra 使用 VN Property System 描述角色、剧情、资产元数据、插件配置、Story Graph 节点和编辑器属性，并生成 schema、序列化、MCP 可编辑字段和属性面板。

### 3.7 现代 C++ 实现

基础容器、字符串、路径、时间、错误处理优先使用 C++ 标准库和成熟第三方库。动态模块 ABI 边界使用 C ABI、opaque handle 和显式 DTO，不跨 ABI 暴露 STL、EnTT、Renderer/Audio 原生句柄或 Editor 内部对象。

### 3.8 Text-First Source Data

项目源数据采用 YAML + JSON Schema。资产、角色、设定、剧情图、本地化、AI 策略、Review Queue、构建配置都应以文本源文件为 canonical source。图片、音频、字体等二进制资源通过同名 `.asset.yaml` sidecar 承载语义元数据，AssetRegistry 由 sidecar 生成。

### 3.9 统一 MCP / Agent 能力层

MCP 是 AstraEngine 的统一 Agent 协议层，而不是只属于编辑器的附属工具。系统分为 `Editor MCP Host` 和 `Runtime MCP Host`：前者服务开发阶段协作、验证、构建和 trusted direct write；后者服务运行时受控内容生成、运行时资源查询和 runtime-safe tool 调用。两者共享 resources、tools、prompts、session 和审计契约，但权限模型不同。Editor MCP Host 默认不进入 packaged runtime；Runtime MCP Host 只有在项目策略和模块权限允许时才可打入发布包。工具副作用与生成来源统一由 Agent Audit 模块记录。

## 4. 当前范围

第一阶段目标不是一次性完成完整 AI VN 工业化平台，而是先形成可运行、可测试、可扩展的核心骨架。

当前应优先实现：

- Core、ApplicationCore、PlatformSDL3。
- Renderer2D、TextCore、AudioCore。
- AssetCore、AssetRegistry、VFS。
- Text-First 源数据 schema 和 asset sidecar。
- ModuleManager、ExtensionRegistry、PluginDescriptor schema 和 VN Property System。
- VNRuntimeServices 和内部 Runtime ECS。
- Astra Runtime、RuntimeCommand 和 Runtime Services facade。
- 统一 MCP / Agent 能力层的 resources、tools、session 和审计设计。
- 最小 Astra DSL 和 RuntimeCommand。
- AstraGame Demo。
- 最小 AstraEditor 或 Headless Preview。

暂缓实现：

- 高自由度 Experimental Runtime Director。
- 高兼容度外部引擎模拟。
- 完整资产市场、云协作、多人编辑。
- 自研复杂 UI 框架，除非现有 UI 技术不能满足编辑器需求。

## 5. 关键设计不变量

- Runtime 不依赖 Editor。
- 普通 AI 生成内容不直接修改 Canonical Project，必须通过 Patch 或 Review Queue；Editor MCP trusted direct write 是显式受信例外。
- Cooked Build 默认确定性，可复现。
- Runtime MCP Host、Runtime Generation Orchestrator 和 runtime-safe Provider 默认关闭，只有项目策略显式开启时才可打入发布包。
- 外部引擎兼容层只能作为动态模块通过 VFS、AssetRegistry、RuntimeCommand、Runtime Services extension、SaveService extension state、Editor/MCP/Cook 扩展接入，不能定义替代运行时或绕过核心状态和存档系统。
- 外部项目默认 Mount-Only，不做 Import，不复制或重打包原始资产。
- 所有资产引用使用 AssetId，不在运行时业务逻辑中散落裸文件路径。
- EnTT 类型不出现在 Runtime Services 对外接口、Editor 接口、Compatibility 插件接口或 AI 插件接口中。
- Canonical source data 使用 YAML + JSON Schema；Cooked、DerivedDataCache、package output 不是 AI/MCP 编辑源。
- Editor MCP trusted direct write 只允许写 workspace/project 内文本源文件，并必须记录 Operation Log。
- Runtime MCP Host 不允许 project_write、未授权外部路径访问或绕过 Save/Replay/Fallback。
- 动态模块必须通过 ModuleManager、ExtensionRegistry 和权限声明接入，不得绕过 Runtime Services、Boundary Manager 或 Release Gate。
- Provider、Generation、Audit 和 Runtime MCP Host 必须作为独立动态模块接入；Packaged runtime 只包含启用且 runtime-safe 的模块，Editor、Developer、Editor MCP 调试模块默认不进入发布包。

## 6. 后续设计决策点

以下内容需要在代码落地前或原型阶段形成 ADR：

- 对外项目名：AstraEngine、AstraEngine 或其他名称。
- RHI 后端：SDL_GPU、bgfx、WebGPU、Vulkan/OpenGL 抽象。
- 编辑器 UI 技术：Dear ImGui、Qt、WebView、原生自绘或混合方案。
- Astra DSL 细节：`.astra` 文本语法、Story Graph YAML 互转、编译 AST 表示和调试信息格式。
- 动态模块签名、分发、版本迁移和编辑器安全卸载策略。
- Runtime Generation Orchestrator 的 prompt snapshot、回放和 fallback 粒度。
- Runtime MCP Host 的 transport 和会话生命周期是否只支持嵌入式 host，还是允许外部 runtime host。

# 术语表

## Actor
运行时公开对象模型，拥有 `ActorId`、`ActorTypeId`、Component 集合、生命周期和状态机。

## ActorId
稳定、可存档、可回放的 Actor 身份，不等同于 ECS entity 或 C++ 指针。

## ComponentDescriptor
组件 schema、默认值、Inspector metadata、序列化和 AI 编辑边界描述。

## Project Template Descriptor
Project Wizard 使用的模板描述，定义模板 ID、runtime profile、默认 provider、seed content、向导字段和验收命令。

## Asset Import Preset
Asset Import Wizard 使用的导入预设，定义 source extensions、asset type、sidecar defaults、cook defaults、license 和 review policy。

## Editor Layout Preset
Editor docking 和命令布局描述，定义 panel id、dock area、visibility、command binding、project default 和 user override。

## Property Details Panel
基于 PropertySystem 和 Component inspector metadata 的通用详情面板，用 typed patch、undo/redo 和 review flags 修改 source。

## Content Browser
创作者管理资产的主入口，支持导入、生成、批量重命名、依赖查看、引用修复、迁移和 Release Gate 诊断。

## Project Wizard / Template Browser
从模板创建项目或 sample 的创作者入口，生成 `.astra.yaml`、Content seed、默认脚本和可验证的初始项目。

## Review Queue
审核 AI draft、patch proposal、导入修改和 release-sensitive edits 的队列。Accepted 后才可写 canonical source 或进入 Cook。

## AI Draft Sidecar
描述创作期 AI draft 的 sidecar，包含 provider、prompt/context/output hash、provenance、review state、license 和 canonical import target。

## Plugin Wizard Template
插件向导模板，生成 plugin descriptor、capability/permission 声明、C ABI stub、sample test、manual stub 和 release checklist。

## Sample Descriptor
样例项目描述，记录 sample id、project path、release profiles、golden replay、commands、acceptance 和 expected evidence。

## Test Matrix
测试覆盖矩阵，按 unit、integration、headless、smoke、stress、compat、release-gate、doc 分类映射样例和命令。

## Implementation Coverage Matrix
设计覆盖索引，要求每个系统都有 design spec、public contract、TODO、validation/release rule、sample/test evidence 和边界。

## EventBus
运行时事件分发系统，传递 RuntimeEvent、VNEvent、PresentationEvent、ScriptEvent 和 AIIntentEvent。

## StateMachineRuntime
驱动 Actor-bound 状态机的运行时。状态机可作为 Component 挂载在 Actor 上。

## ControlPolicy
Actor 控制权组件，处理优先级、锁定 channel、打断、排队和拒绝。

## Director
全局叙事仲裁者，管理剧情阶段、Timeline lock、AI 可用范围和 legacy VM 同步约束。

## PresentationCommand
状态机内部输出到底层表现服务的命令，例如创建文本框、播放语音、切换表情、启动滤镜。

## AstraVN / Presentation.VN
视觉小说垂直模块，提供 VN DSL、VN Event、Dialogue、Choice、Character、Background 和预定义状态机。

## ScriptRuntimeHost
管理 Astra Native、Lua、BGI、Kirikiri、Custom 等脚本运行时的宿主。

## FilterGraph
统一后处理和现代化管线，支持 per-layer filter 和 final-screen filter。

## FilterProfile
文本源资产，描述滤镜 pass、目标层、参数和现代化配置。

## AIIntent
AI 运行时输出的结构化意图，只能经 Validator、ControlPolicy 和 Director 审核后执行。

## Runtime AI MCP
运行时 MCP Host 提供的受控生成链路，根据玩家反馈和 runtime context 生成 `AIIntent`，
经验证后提交为 deterministic committed output。

## Editor Copilot MCP
创作期 Copilot 式辅助链路，提供建议、解释、patch proposal、测试/Cook/Release Gate 辅助。

## Editor Content Generation MCP
创作期内容生成、修改和增强链路，产出 draft，经 Review Queue 接受后进入 canonical source。

## AI Draft
AI 在创作期生成的临时内容，包括文本、图像、音频、语音、视频、动画、FilterProfile 或 metadata。
Draft 未 accepted 前不进入 AssetRegistry、Cook 或 Package。

## Committed AI Output
Runtime AI 经验证后提交的确定性输出。存档和回放使用 committed output，不重新请求 Provider。

## RuntimeGenerationOrchestrator
运行时生成编排器。它构建 runtime context，调用 Provider，生成 `AIIntent`，并把结果交给 `IntentValidator`、ControlPolicy 和 Director。

## IntentValidator
校验 `AIIntent` 是否满足角色在场、剧情阶段、Canon、权限、分级、Timeline lock 和 ControlPolicy 约束。

## Agent Audit
记录工具副作用和生成来源的审计系统，分 Operation Log 与 Generation Audit Log。

## AgentAudit
Agent Audit 的运行时/模块接口名，用于注册 audit sink、写入 Operation Log 和 Generation Audit Log。

## Legacy VM
旧 VN 引擎脚本或 bytecode 的模拟运行时，例如 BGI VM、Kirikiri runtime。

## API Mapper
把 legacy VM 图像、文本、音频、变量和系统调用映射为 Astra RuntimeEvent 或 PresentationCommand 的适配层。

## Modernization Profile
旧游戏现代化配置，包括字体替换、UI 覆盖、FilterProfile、缩放策略、高清资源覆盖和本地化覆盖。

## ServiceRegistry
模块获取引擎服务的注册表，返回最小 public service interface 或 opaque handle。

## ExtensionRegistry
模块注册扩展能力的注册表，例如 Actor type、StateMachine、ScriptRuntime、Filter、Provider、CompatRuntime 和 Editor panel。

## PropertySystem
轻量类型和属性描述系统，用于 schema、Inspector、MCP 字段编辑、序列化和插件配置。

## Text-First Source
以 YAML + JSON Schema 为 canonical source 的项目格式；二进制资产语义写在 `.asset.yaml` sidecar 中。

## AssetRegistry
由 sidecar、source hash、dependency graph 和 importer/cook metadata 生成的资产注册表。它不是人工或 AI 编辑源。

## DerivedDataCache / DDC
Cook 产物缓存，按 source hash、cook preset、processor/provider version、platform 和 release profile 生成 key。DDC 不是 source。

## Package Manifest
发布包清单，记录 cooked asset table、dependency table、runtime-safe module list、EngineModuleSlot selection、hash 和 release profile。

## Release Gate
发布门禁，根据 profile 和 machine-readable diagnostics 阻止 invalid schema、missing dependency、unreviewed AI draft、invalid plugin permission、runtime AI policy violation 等问题进入包。

## Astra CLI
命令行工具入口，提供 validate、import、cook、package、run、replay、inspect、doc-check 和 plugin validate。

## Trace
运行时可观测事件流，记录 frame、event dispatch、scheduler、script、asset load、media backend、AI intent、module lifecycle 等通道。

## Crash Bundle
崩溃/错误报告包，包含 build info、diagnostics、last logs、last trace frames、runtime summary、package/module summary 和可选平台 minidump。

## Creator Experience Parity
在 Astra 的 2D/VN-first 范围内达到 UE 级创作者友好度：模板、Content Browser、Inspector、
Graph/Timeline、PIE、Runtime Debugger、Cook/Package 和 Release Gate 形成闭环。

## Customization Parity
通过插件、EngineModuleSlot、Provider、Editor panel、MCP tool 和 Property schema 提供 UE 级可定制度。

## EngineModuleSlot
项目可显式选择 provider 的能力槽位，例如 Renderer2D、TextLayout、Audio、ScriptRuntime、PresentationLibrary 或 Editor panel。

## IEditorPanelProvider
Editor panel 扩展 contract，声明 panel id、菜单、命令、context action、required services 和 layout defaults。

## IAssetImporter
资产导入扩展 contract，声明 source extensions、asset type、preset schema、sidecar defaults 和 diagnostics。

## ICookProcessor
Cook 扩展 contract，声明 input asset type、output artifact、DDC key、package eligibility 和 release gate rule。

## IScriptRuntimeProvider
脚本运行时 provider contract，声明 runtime id、source types、host API、debug hook 和 snapshot capability。

## IPresentationLibraryProvider
Presentation/VN 扩展 contract，声明 command/event kinds、状态机、preview 支持和 package eligibility。

## IRenderer2DProvider / ITextLayoutProvider / IAudioProvider
Media backend 替换 contract，声明 slot id、backend features、headless support、native handle 隔离和 packaged eligibility。

## IMcpToolProvider
MCP 工具扩展 contract，声明 resources、tools、prompts、session requirement、mutating behavior 和 audit policy。

## IAIProvider
AI provider contract，声明 modality、network/offline、runtime eligibility、secret requirements、streaming 和 audit 支持。

## DecodeProvider
媒体解码 provider，按 `astra.image_decode`、`astra.audio_decode`、`astra.video_decode` slot 注册。它声明 codec/container/profile、硬件加速、zero-copy、CPU fallback、headless support 和 packaged eligibility。

## MediaSurfaceToken
Decode Provider 返回给兼容 Renderer2D provider 的 backend-scoped opaque surface token。它不可序列化，不暴露 D3D、Vulkan、Metal、SDL、OS 或平台视频解码 handle。

## ProviderCapability
Provider 声明并由 Release Gate 验证的能力项，例如 codec support、zero-copy、frame capture、hot reload level、headless fallback 或 package eligibility。

## ReleaseProfile
发布配置，决定 blocking severity、fallback 允许策略、runtime AI policy、provider hash/ABI 要求、save migration 要求和 package inclusion policy。

## ReplayMismatch
Replay 比较失败记录，定位 frame、record kind、event sequence、actor/component、script command、presentation command 或 provider output hash 的差异。

## EditorRuntimeSession
Editor 与 RuntimeWorld 的调试/PIE 会话边界。它提供 inspect、debug command、preview overlay 和 source patch proposal，不让 Runtime 依赖 Editor UI。

## SaveSectionProvider
模块注册 save section 的 provider contract，负责声明 section schema、写入 payload、读取 payload 和提供 migration edge。

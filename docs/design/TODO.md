# TODO

状态：Draft  
依据：[README.md](README.md)、[architecture.md](architecture.md)、[extension-and-module-system.md](extension-and-module-system.md)、[ai-collaboration.md](ai-collaboration.md)、[editor-and-pipeline.md](editor-and-pipeline.md)、[compatibility-layer.md](compatibility-layer.md)、[roadmap.md](roadmap.md)

## 1. 执行约定

优先级：

- P0：阻塞后续工作的基础任务。
- P1：当前阶段核心功能。
- P2：增强功能或可延后任务。
- P3：实验性或长期任务。

状态：

- `[ ]` 未开始。
- `[x]` 已完成。

完成定义：

- 有代码或文档落地。
- 有最小测试或可手动验证路径。
- 不违反 Runtime 不依赖 Editor、AI 不直接改 Canon、Cooked Build 默认确定性这些设计不变量。

## 2. Phase 0：仓库与设计基线

目标：形成可协作、可配置、可扩展的工程骨架。

### 2.1 命名与决策记录

- [ ] P0 建立 `docs/adr` 目录。
- [ ] P0 编写 ADR：动态模块优先、C ABI 边界、VN Property System，不引入完整 UObject。
- [ ] P0 更新 ADR 0001 为历史性 bootstrap 决策，由动态模块优先 ADR supersede。
- [ ] P1 编写 ADR：编辑器 UI 技术候选和第一阶段选择。
- [ ] P1 编写 ADR：Renderer2D 第一阶段后端选择，候选为 SDL_GPU、bgfx、WebGPU 或 OpenGL。
- [ ] P1 编写 ADR：Runtime Services 内部采用 EnTT + Bevy 风格 ECS。
- [ ] P1 编写 ADR：MCP 作为 Editor/Developer 插件，trusted session 允许直接写入并记录 Operation Log。
- [ ] P1 编写 ADR：源数据采用 YAML + JSON Schema，二进制资源使用 `.asset.yaml` sidecar。
- [ ] P1 编写 ADR：兼容/模拟通过动态模块和现有扩展机制实现，不做 Import Mode。

验收标准：

- 新增 ADR 有编号、状态、背景、决策、后果。
- `docs/design/README.md` 中的命名说明与 ADR 一致。

### 2.2 工程骨架

- [ ] P0 创建顶层 `CMakeLists.txt`。
- [ ] P0 创建 `vcpkg.json`。
- [ ] P0 创建 `Engine/Runtime`、`Engine/Editor`、`Engine/Developer`、`Engine/Programs`、`Engine/Plugins`。
- [ ] P0 创建 `Projects/Samples/MinimalVN` 样例项目目录。
- [ ] P0 创建 `cmake` 辅助模块目录。
- [ ] P0 创建 `.gitignore`，排除 build、Saved、DerivedDataCache、临时日志和本地密钥。
- [ ] P1 创建 `docs/coding-style.md`，明确 C++ 标准、命名、错误处理、模块边界。
- [ ] P1 创建 `docs/design/content-and-assets.md`，明确 Text-First 源数据和 asset sidecar。
- [ ] P1 创建 `docs/design/mcp-integration.md`，明确 MCP resources、tools、prompts、trusted session 和 Operation Log。
- [ ] P1 创建 `docs/design/extension-and-module-system.md`，明确 ModuleManager、ExtensionRegistry、AstraModule C ABI、PluginDescriptor、VN Property System、权限和打包规则。

验收标准：

- `cmake -S . -B build` 能完成空工程配置。
- `vcpkg.json` 包含 SDL3、FreeType、HarfBuzz、fmt、spdlog、nlohmann-json、yaml-cpp、glm、EnTT、miniaudio、Catch2。
- 设计文档明确 canonical source data 是 YAML + JSON Schema。
- 设计文档明确 AssetRegistry 从 `.asset.yaml` sidecar 生成。
- 设计文档明确动态模块是默认扩展模型，源码级模块仅用于核心或实验性内部代码。

### 2.3 基础 CI 与质量门槛

- [ ] P1 添加格式化配置，优先 `clang-format`。
- [ ] P1 添加基础 CI：配置 CMake、构建、运行测试。
- [ ] P1 添加 `CTest` 支持。
- [ ] P2 添加静态检查配置，候选为 `clang-tidy`。

验收标准：

- CI 至少覆盖 Debug 构建。
- 测试失败时 CI 阻塞合并。

## 3. Phase 1：基础 Runtime

目标：能通过 Astra Runtime 运行一个确定性 Astra VN Demo。

### 3.1 Core

- [ ] P0 创建 `Astra_Core` target。
- [ ] P0 实现日志封装，底层可用 spdlog。
- [ ] P0 实现断言和 fatal error 入口。
- [ ] P0 定义 `Expected` 使用策略，优先 `std::expected`。
- [ ] P0 定义基础错误类型和错误码命名规范。
- [ ] P0 实现路径工具，统一 `std::filesystem::path` 与 UTF-8 字符串边界。
- [ ] P1 实现 YAML 配置加载，并接入 JSON Schema 校验。
- [ ] P1 实现时间工具，基于 `std::chrono`。
- [ ] P1 实现事件总线最小版本。
- [ ] P1 实现模块注册接口。
- [ ] P2 实现任务系统最小版本。
- [ ] P2 实现轻量类型描述或反射元数据。

验收标准：

- Core 不依赖 SDL、Renderer、Audio、Editor。
- Core 有单元测试覆盖日志初始化、路径规范化、配置加载、错误返回。

### 3.1.1 Dynamic Module / ExtensionRegistry / VN Property System

- [ ] P0 创建 `Astra_ModuleRuntime` target。
- [ ] P0 创建 `Astra_ExtensionRegistry` target。
- [ ] P0 创建 `Astra_VNPropertySystem` target。
- [ ] P0 定义 `PluginDescriptor` YAML schema。
- [ ] P0 定义 `AstraModule` C ABI entrypoint。
- [ ] P0 定义 ABI 基础类型：result code、opaque handle、string view、diagnostic sink、host api table。
- [ ] P0 实现 `ModuleManager` 的插件目录扫描和 descriptor 校验。
- [ ] P0 实现模块依赖解析、load phase 排序和版本约束校验。
- [ ] P1 实现动态库加载、initialize、activate、deactivate、shutdown、unload 生命周期。
- [ ] P1 实现 `ExtensionRegistry` 的扩展点注册和重复注册诊断。
- [ ] P1 定义第一阶段扩展点：RuntimeCommandSource、CompatibilityAdapter、VfsMountProvider、ForeignAssetResolver、AssetValidator、CookProcessor、EditorPanelProvider、McpProvider、AIProvider。
- [ ] P1 实现模块权限声明和 capability 校验。
- [ ] P1 实现 VN Property System：TypeId、PropertyId、enum metadata、property descriptor、默认值。
- [ ] P1 实现 VN Property System 到 JSON Schema 的生成。
- [ ] P1 实现 `ai_editable`、`tool_generated`、`read_only`、`requires_review` 字段标记。
- [ ] P1 创建示例动态模块，注册一个 RuntimeCommandSource 或测试扩展。
- [ ] P2 实现编辑器安全 unload/reload 开发路径。
- [ ] P2 实现插件 SDK 包装层，但稳定边界仍是 C ABI。

验收标准：

- 示例动态模块可被发现、校验、加载、注册扩展、停用并卸载。
- ABI 边界不暴露 STL、EnTT、Renderer2D、AudioCore、PlatformSDL3 或 Editor 内部对象。
- 版本不匹配、缺失依赖、重复扩展和权限不足都有明确诊断。
- 插件定义的 VN property type 可生成 JSON Schema 并被 YAML 校验使用。

### 3.2 ApplicationCore / PlatformSDL3

- [ ] P0 创建 `Astra_ApplicationCore` target。
- [ ] P0 创建 `Astra_PlatformSDL3` target。
- [ ] P0 实现 SDL3 初始化和关闭。
- [ ] P0 实现窗口创建、销毁、大小变更。
- [ ] P0 实现主循环和事件泵。
- [ ] P1 实现键盘、鼠标、手柄输入事件转换。
- [ ] P1 实现剪贴板封装。
- [ ] P1 实现高精度时间查询。
- [ ] P1 实现平台路径查询，如用户目录、存档目录、缓存目录。
- [ ] P2 实现触摸输入。
- [ ] P2 实现多显示器信息查询。

验收标准：

- 上层模块不直接暴露 SDL 类型。
- 最小窗口程序能启动、响应关闭事件并稳定退出。

### 3.3 RHI / Renderer2D

- [ ] P0 确定第一阶段 RHI 后端。
- [ ] P0 创建 `Astra_RHI` target。
- [ ] P0 创建 `Astra_Renderer2D` target。
- [ ] P0 定义纹理、缓冲、着色器、命令提交的最小抽象。
- [ ] P1 实现图片纹理加载。
- [ ] P1 实现 Sprite 绘制。
- [ ] P1 实现图层排序。
- [ ] P1 实现 alpha 混合。
- [ ] P1 实现背景绘制。
- [ ] P1 实现立绘绘制。
- [ ] P1 实现 UI 矩形和基础九宫格。
- [ ] P2 实现转场系统，至少 fade。
- [ ] P2 实现截图和缩略图。
- [ ] P2 实现 debug overlay。
- [ ] P3 实现粒子和后处理。

验收标准：

- Demo 能显示背景和立绘。
- Renderer2D 不依赖 VN DSL 或 Editor。
- 渲染帧流程符合 `architecture.md` 中定义的 pass 顺序。

### 3.4 TextCore

- [ ] P0 创建 `Astra_TextCore` target。
- [ ] P0 接入 FreeType。
- [ ] P0 接入 HarfBuzz。
- [ ] P1 实现字体加载和 fallback。
- [ ] P1 实现 glyph atlas。
- [ ] P1 实现基础文本 shaping 和 layout。
- [ ] P1 实现自动换行。
- [ ] P1 实现富文本 span 数据结构。
- [ ] P1 实现描边和阴影。
- [ ] P1 实现打字机效果所需的 layout 可见范围。
- [ ] P2 实现标点避头尾。
- [ ] P2 实现 ruby / furigana。
- [ ] P2 实现 emoji fallback。
- [ ] P2 实现文本历史所需的数据结构。

验收标准：

- 能稳定渲染中文、英文、日文混排。
- 相同文本和字体输入可复用 layout 缓存。
- 有文本 layout 单元测试或 golden data。

### 3.5 AudioCore

- [ ] P0 创建 `Astra_AudioCore` target。
- [ ] P0 选择音频后端，默认候选 miniaudio。
- [ ] P1 实现 BGM 播放和停止。
- [ ] P1 实现 SFX 播放。
- [ ] P1 实现 Voice 播放。
- [ ] P1 实现音量总线：Master、BGM、SFX、Voice。
- [ ] P1 实现淡入淡出。
- [ ] P2 实现音频流播放。
- [ ] P2 实现 TTS 预览缓存路径规则。
- [ ] P2 实现音频资源热重载钩子。

验收标准：

- Demo 能播放 BGM 和 SFX。
- BGM fade 不阻塞主线程。
- 音频模块不依赖 Editor。

### 3.6 AssetCore / AssetRegistry / VFS

- [ ] P0 创建 `Astra_AssetCore` target。
- [ ] P0 创建 `Astra_AssetRegistry` target。
- [ ] P0 创建 `Astra_VFS` target。
- [ ] P0 定义 `AssetId` 语法和解析器。
- [ ] P0 定义 `AssetType`、`AssetMetadata`、`ContentOrigin`。
- [ ] P0 定义 `.asset.yaml` sidecar schema。
- [ ] P0 实现普通目录 mount。
- [ ] P1 实现 YAML sidecar 解析。
- [ ] P1 实现 JSON Schema 校验。
- [ ] P1 实现 AssetRegistry 生成格式。
- [ ] P1 实现资产扫描。
- [ ] P1 实现按 ID、类型、标签查询。
- [ ] P1 实现依赖记录。
- [ ] P1 实现图片资产加载。
- [ ] P1 实现音频资产加载。
- [ ] P2 实现 ZIP mount。
- [ ] P2 实现补丁包 mount 优先级。
- [ ] P2 实现缺失资产诊断。
- [ ] P2 实现重复 AssetId 诊断。
- [ ] P2 实现 sidecar/source_path 不一致诊断。
- [ ] P2 实现 external asset metadata 解析。
- [ ] P2 实现 `foreign-director`、`foreign-renpy` 等 external AssetId scheme。
- [ ] P2 实现 mount-only asset copy policy 诊断。
- [ ] P3 实现 PAK、XP3、RPA、NSA mount。

验收标准：

- 所有 Runtime 业务逻辑使用 `AssetId`，不散落裸文件路径。
- Demo 资产能通过 sidecar 生成的 AssetRegistry 加载。
- VFS mount 顺序可测试。
- Humans、AI、MCP 编辑 `.asset.yaml`，不直接编辑生成 registry。
- External assets 可进入 AssetRegistry 索引，但默认不复制到 cooked output。

### 3.6.1 Text-First Source Data

- [ ] P0 定义 YAML + JSON Schema 作为 canonical source format。
- [ ] P0 定义 project manifest schema：`*.vnproj.yaml`。
- [ ] P0 定义 config schema：`Config/*.yaml`。
- [ ] P0 定义 character schema：`*.character.yaml`。
- [ ] P0 定义 lore schema：`*.lore.yaml`。
- [ ] P0 定义 story graph schema：`*.story.yaml`。
- [ ] P0 定义 localization schema：`*.loc.yaml`。
- [ ] P0 定义 review queue schema：`*.review.yaml`。
- [ ] P0 定义 audit schema：`*.audit.yaml`。
- [ ] P0 定义 plugin descriptor schema：`*.plugin.yaml`。
- [ ] P1 实现稳定 ID 校验。
- [ ] P1 实现 duplicate ID 校验。
- [ ] P1 实现 AI-editable、tool-generated、read-only 字段标记。
- [ ] P1 实现 YAML block scalar 使用规范文档。
- [ ] P2 实现 schema migration 策略文档。

验收标准：

- 所有源数据类型都有 schema。
- PluginDescriptor 可通过 schema 校验 module type、load phase、capability、permission、platform 和 dependency 字段。
- Release Gate 能发现 YAML 解析错误和 schema mismatch。
- AI/MCP 可通过稳定 ID 定位资产、角色、设定、剧情图、本地化条目。

### 3.7 VNRuntimeServices

- [ ] P0 创建 `Astra_VNRuntimeServices` target。
- [ ] P0 定义 `RuntimeCommand`。
- [ ] P0 定义 `RuntimeCommandExecutor`。
- [ ] P1 定义并实现 `StageService`。
- [ ] P1 定义并实现 `DialogueService`。
- [ ] P1 定义并实现 `ChoiceService`。
- [ ] P1 定义并实现 `AudioService` facade。
- [ ] P1 定义并实现 `AssetService` facade。
- [ ] P1 定义并实现 `InputService`。
- [ ] P1 定义并实现 `SaveService` 最小版本。
- [ ] P2 定义并实现 `LocalizationService` 最小版本。
- [ ] P2 实现 Runtime Command Log。

验收标准：

- Astra Runtime 只通过 Runtime Services 驱动舞台、对白、选择、音频和存档。
- Headless Test 可替换渲染和音频实现。
- EnTT 类型不出现在 Runtime Services 对外接口。
- RuntimeCommand 是 Astra Runtime、Story Graph、Runtime AI 和 compatibility adapter 进入 Runtime Services 的稳定意图协议和可选日志格式。

### 3.7.1 VNRuntimeServices / ECS

- [ ] P0 在 `Astra_VNRuntimeServices` 内建立 `ECS` 子模块，第一阶段不单独拆 target。
- [ ] P0 封装 Runtime ECS World，底层使用 EnTT。
- [ ] P0 定义 `CommandBuffer`，用于 RuntimeCommand 到 World 的延迟写入。
- [ ] P0 定义固定 Schedule：Input、Script、CommandApply、Animation、Audio、RenderExtract、SaveSnapshot、Cleanup。
- [ ] P1 定义组件：`Transform2DComponent`、`SpriteComponent`、`BackgroundComponent`、`DialogueComponent`、`ChoiceComponent`、`AudioRequestComponent`、`LifetimeComponent`、`TransitionComponent`。
- [ ] P1 定义资源：`AssetRegistryResource`、`InputResource`、`SaveStateResource`、`DialogueHistoryResource`、`AudioBusResource`、`RuntimeConfigResource`。
- [ ] P1 将 `RuntimeCommandExecutor` 改为写入 CommandBuffer、World 或 Resource。
- [ ] P1 将 `StageService` 改为 ECS facade，不再持有与 World 分叉的权威舞台状态。
- [ ] P1 将 `DialogueService` 改为 ECS facade，当前对白为组件，历史为资源。
- [ ] P1 将 `AudioService` 改为写入音频请求，由 Audio 阶段消费。
- [ ] P1 将 `SaveService` 改为从 World + Resources 生成确定性快照。
- [ ] P2 实现 RenderExtract 系统，输出 Renderer2D 可消费的快照。
- [ ] P2 实现 Audio 系统，输出 AudioCore 可消费的播放请求。
- [ ] P2 实现 Cleanup 系统，清理短生命周期实体。

验收标准：

- ECS World 由 Runtime Services 拥有，对外只暴露引擎自有 DTO、RuntimeCommand、Runtime Services facade 和 extension API。
- 同一个 MinimalVN Demo 在 ECS 重构前后输出等价 RuntimeCommand Log。
- Headless Test 不初始化 Renderer2D 或 AudioCore 也能运行 schedule。
- Save/Load 不依赖 EnTT entity 原始值稳定性。

### 3.7.2 Astra Runtime Session / Extension API

- [ ] P0 创建 `Astra_AstraRuntime` target。
- [ ] P0 定义 Astra Runtime session 生命周期：loadProject、start、tick、submitInput、saveSnapshot、restoreSnapshot、shutdown。
- [ ] P0 定义 Runtime extension API，允许动态模块注册 RuntimeCommandSource、Runtime Services extension、Runtime ECS system pack 和 SaveService extension state provider。
- [ ] P1 实现 RuntimeCommandSource 调度顺序和诊断。
- [ ] P1 实现 SaveService extension state snapshot。
- [ ] P1 实现 project config：`compatibility.external_project_root`、`compatibility.mount_only`、`compatibility.allow_asset_copy`。
- [ ] P2 实现 mock compatibility module 测试夹具。

验收标准：

- AstraGame、PIE 和 Headless Test 使用同一 Astra Runtime session。
- 动态模块不能替代 Astra Runtime 主循环。
- 动态模块不直接访问 EnTT、Renderer2D、AudioCore、PlatformSDL3 或 Editor。
- Runtime extension API 返回引擎自有 DTO，不暴露内部 handle。

### 3.8 Astra Runtime

- [ ] P0 定义最小 Astra DSL AST。
- [ ] P0 实现 Astra Runtime。
- [ ] P0 实现场景、背景、立绘、对白、选择、变量、跳转语法。
- [ ] P1 实现 DSL parser。
- [ ] P1 实现 AST 到 RuntimeCommand 的 planner。
- [ ] P1 实现变量系统。
- [ ] P1 实现选择分支。
- [ ] P1 实现场景状态。
- [ ] P1 实现保存快照。
- [ ] P2 实现 Story Graph 的运行时数据结构。
- [ ] P2 实现 Agent Hook 占位节点，但默认不启用 AI。

验收标准：

- 最小 DSL Demo 能跑通。
- 选择分支能改变变量并跳转。
- 保存读取后能恢复当前对白和变量。

### 3.9 AstraGame 与 Demo

- [ ] P0 创建 `AstraGame` executable。
- [ ] P0 创建 `Projects/Samples/MinimalVN`。
- [ ] P1 添加最小背景、立绘、BGM、SFX、脚本样例。
- [ ] P1 实现命令行启动项目路径。
- [ ] P1 实现基础窗口和主循环。
- [ ] P1 接入 Astra Runtime session。
- [ ] P1 接入 SaveGame。
- [ ] P2 添加 debug overlay 显示当前 scene、line、fps。

验收标准：

- 运行 `AstraGame --project Projects/Samples/MinimalVN` 可进入 Demo。
- Demo 至少包含一处选择分支和一次存档读取验证。

## 4. Phase 2：Editor 基础

目标：能创建、编辑、预览最小 VN 项目。

### 4.1 Editor Core

- [ ] P0 创建 `Astra_EditorCore` target。
- [ ] P0 创建 `AstraEditor` executable。
- [ ] P1 实现编辑器应用生命周期。
- [ ] P1 实现项目打开和关闭。
- [ ] P1 实现最近项目列表。
- [ ] P1 实现 Output Log。
- [ ] P2 实现编辑器设置保存。

验收标准：

- Editor 依赖 Runtime，Runtime 不依赖 Editor。
- 空项目可打开并显示基础 UI。

### 4.2 Project Browser

- [ ] P1 实现创建项目向导。
- [ ] P1 实现打开已有项目。
- [ ] P1 生成 `*.vnproj.yaml`。
- [ ] P1 生成默认 `Config` 和 `Content` 目录。
- [ ] P2 支持项目模板。

验收标准：

- 新项目可被 AstraGame 启动。
- 项目配置能被编辑器和运行时读取。

### 4.3 Content Browser / AssetTools

- [ ] P1 实现目录树和资产列表。
- [ ] P1 实现资产导入。
- [ ] P1 实现资产重命名。
- [ ] P1 实现 AssetMetadata 编辑。
- [ ] P1 实现 `.asset.yaml` sidecar 创建和编辑。
- [ ] P1 实现图片、音频预览。
- [ ] P2 实现资产标签编辑。
- [ ] P2 实现依赖查看。
- [ ] P2 实现资产重新导入。

验收标准：

- 导入图片后能在 Scene Preview 中引用。
- 导入音频后能在 Runtime 中播放。
- 导入二进制资源时能生成同名 sidecar。

### 4.4 Script Editor

- [ ] P1 实现文本编辑器基础功能。
- [ ] P1 实现 VN DSL 语法高亮。
- [ ] P1 实现语法错误显示。
- [ ] P1 实现保存后触发 parser 验证。
- [ ] P2 实现跳转到 scene 和 label。
- [ ] P2 实现 RuntimeCommand 预览。

验收标准：

- 编辑脚本后可直接从当前 scene 启动 PIE。
- 语法错误不导致编辑器崩溃。

### 4.5 Scene Preview / Play In Editor

- [ ] P1 实现 Scene Preview 面板。
- [ ] P1 实现从开头运行。
- [ ] P1 实现从当前 Scene 运行。
- [ ] P1 实现从当前 Story Node 运行的接口占位。
- [ ] P1 实现 Runtime Command Log 面板。
- [ ] P2 实现变量注入启动。
- [ ] P2 实现模拟玩家选择。
- [ ] P2 实现 Packaged Preview 占位。

验收标准：

- PIE 与 AstraGame 使用同一 VNRuntimeServices。
- PIE 能显示背景、立绘、对白、选择。

## 5. Phase 3：AI Suggestion Layer

目标：AI 可以辅助创作，但不能直接覆盖正式内容。

### 5.1 AI 基础接口

- [ ] P0 创建 `Astra_AIRuntime` target。
- [ ] P0 定义 `IAIProvider`。
- [ ] P0 定义 `AIRequest`、`AIResponse`、`AIError`。
- [ ] P1 定义 Provider 权限声明。
- [ ] P1 创建 `OpenAIProvider` 插件骨架。
- [ ] P1 创建 `LocalLLMProvider` 插件骨架。
- [ ] P2 支持 streaming response。

验收标准：

- 没有 AI Provider 时项目仍可完整运行。
- Runtime Build 默认不包含联网 AI Provider。

### 5.2 Boundary Manager

- [ ] P0 定义 Project AI Policy schema。
- [ ] P0 定义 Stage AI Policy schema。
- [ ] P0 实现策略加载。
- [ ] P1 实现 `allow_ai_modify_canon` 检查。
- [ ] P1 实现 `require_human_approval` 检查。
- [ ] P1 实现 `allow_runtime_generation` 检查。
- [ ] P1 实现 Canon Lock 检查。
- [ ] P1 实现权限结果和拒绝原因。
- [ ] P2 添加策略单元测试矩阵。

验收标准：

- AI 无法直接修改 locked Canon Lore。
- 策略拒绝时 UI 能显示明确原因。

### 5.3 Context Builder

- [ ] P1 定义上下文包格式。
- [ ] P1 支持选择目标脚本上下文。
- [ ] P1 支持关联角色卡。
- [ ] P1 支持关联 Canon Lore。
- [ ] P1 支持上下文 hash。
- [ ] P2 支持敏感备注脱敏。
- [ ] P2 支持最小必要上下文裁剪。

验收标准：

- 相同输入可生成相同 `context_hash`。
- Context Builder 不泄露被策略禁止的内容。

### 5.4 Diff / Patch

- [ ] P0 定义 Patch schema。
- [ ] P1 实现对白替换 patch。
- [ ] P1 实现对白新增 patch。
- [ ] P1 实现 lore suggestion patch。
- [ ] P1 实现 patch stale 检查。
- [ ] P1 实现 patch 应用和回滚。
- [ ] P2 实现大 patch 拆分。
- [ ] P2 实现可视化 diff 数据结构。

验收标准：

- AI 输出不能绕过 patch 直接写入 Canonical Project。
- 源内容已变化时 patch 应用会被阻止。

### 5.5 Review Queue

- [ ] P0 定义 Review Queue 存储格式。
- [ ] P1 实现 Accept。
- [ ] P1 实现 Edit。
- [ ] P1 实现 Reject。
- [ ] P1 实现 Defer。
- [ ] P1 实现按类型过滤。
- [ ] P2 实现 Split。
- [ ] P2 实现团队协作所需的冲突提示。

验收标准：

- 接受、编辑、拒绝都会写入审计日志。
- 未审核 AI 内容可被 Release Gate 发现。

### 5.6 Provenance / Audit Log

- [ ] P0 定义 Audit Event schema。
- [ ] P1 实现追加写入审计日志。
- [ ] P1 记录 agent、model、target、context_hash、output_hash。
- [ ] P1 记录作者动作。
- [ ] P1 生成 AI Content Audit 报告。
- [ ] P2 支持按内容 ID 查询来源。
- [ ] P2 支持导出脱敏报告。

验收标准：

- 发布前能统计 AI 辅助内容数量。
- 审计日志不保存 API Key。

### 5.7 Agent Workbench / Prompt Studio

- [ ] P1 创建 Agent Workbench 面板。
- [ ] P1 创建 Prompt Studio 面板。
- [ ] P1 支持运行对白润色 Agent。
- [ ] P1 支持预览 AI 输出。
- [ ] P1 支持把输出发送到 Review Queue。
- [ ] P2 支持 Agent Evaluation。
- [ ] P2 支持批量生成但全部进入 Review Queue。

验收标准：

- AI 生成对白建议进入 Review Queue。
- Agent Workbench 不能直接写正式脚本。

### 5.8 MCP Integration

- [ ] P0 定义 MCP hosting：Editor/Developer 插件，默认禁用。
- [ ] P0 定义 trusted session lifecycle。
- [ ] P0 定义 MCP Operation Log schema。
- [ ] P0 定义 workspace/project path boundary。
- [ ] P1 定义 resources：project manifest、config、assets registry、asset metadata、scripts、story graph、lore、characters、localization、review queue、audit log、build status。
- [ ] P1 定义 tools：project.open、project.inspect、project.write_file、asset.query、asset.write_sidecar、asset.validate_sidecars、script.validate、script.write、story.validate_graph、story.write_graph、lore.write、character.write、localization.write、review.enqueue、audit.generate_ai_report、test.run_headless、compat.probe_project、compat.validate_mount、compat.inspect_assets、compat.inspect_scripts、compat.validate_modernization、compat.generate_diagnostics、build.cook、build.package、release.run_gate。
- [ ] P1 定义 prompts：dialogue polish、lore consistency、character OOC、localization draft、QA route analysis。
- [ ] P1 设计 mutating tools 的 direct write 行为。
- [ ] P1 设计 mutating tools 的 Operation Log 写入。
- [ ] P1 设计 secret redaction 和外部路径拒绝。
- [ ] P2 设计 `AstraMCPServer` 程序封装。

验收标准：

- MCP 不进入默认 packaged runtime。
- MCP 不暴露 EnTT/ECS 内部状态。
- Trusted direct write 可写文本源文件并记录 Operation Log。
- MCP tools 能覆盖验证、headless test、compat probe/mount/assets/scripts/modernization diagnostics、cook、package、release gate。

## 6. Phase 4：完整 VN Authoring

目标：形成完整 VN 制作套件。

### 6.1 StoryGraphEditor

- [ ] P1 定义 Story Graph 文件格式。
- [ ] P1 实现 Scene Node。
- [ ] P1 实现 Dialogue Node。
- [ ] P1 实现 Choice Node。
- [ ] P1 实现 Condition Node。
- [ ] P1 实现 Ending Node。
- [ ] P2 实现 Agent Generation Node。
- [ ] P2 实现 Subroutine Node。
- [ ] P2 实现 Story Graph 到执行计划编译。

验收标准：

- Story Graph 能生成可运行 RuntimeCommand 序列。
- 未连接节点能被验证器报告。

### 6.2 CharacterEditor / LoreEditor

- [ ] P1 定义角色卡 schema。
- [ ] P1 实现角色基础信息编辑。
- [ ] P1 实现角色口吻和标签编辑。
- [ ] P1 定义 Lore schema。
- [ ] P1 实现 Canon Lock 编辑。
- [ ] P1 实现 lore 引用查询。
- [ ] P2 实现 OOC 检测接口占位。
- [ ] P2 实现设定矛盾检测接口占位。

验收标准：

- AI 可引用允许引用的 Canon Lore。
- locked lore 变更必须走 Review Queue。

### 6.3 SceneEditor

- [ ] P1 实现背景选择。
- [ ] P1 实现立绘槽位和站位编辑。
- [ ] P1 实现 BGM 选择。
- [ ] P1 实现转场配置。
- [ ] P2 实现镜头和图层 transform 编辑。
- [ ] P2 实现分镜建议接口占位。

验收标准：

- SceneEditor 输出能被 RuntimeCommandExecutor 执行。
- 场景预览与运行时表现一致。

### 6.4 LocalizationEditor

- [ ] P1 定义本地化 key 格式。
- [ ] P1 实现文本抽取。
- [ ] P1 实现翻译表编辑。
- [ ] P1 实现缺失 key 检查。
- [ ] P2 实现术语表。
- [ ] P2 实现文本溢出检测。
- [ ] P2 实现 AI 翻译草稿进入 Review Queue。

验收标准：

- 能生成至少 `zh-CN` 和 `en-US` 的本地化表。
- 富文本标签不匹配会被报告。

### 6.5 QA / Eval Lab

- [ ] P1 实现分支覆盖扫描。
- [ ] P1 实现死分支检测。
- [ ] P1 实现未使用资产检测。
- [ ] P1 实现缺失资产检测。
- [ ] P2 实现剧情矛盾检查接口占位。
- [ ] P2 实现 OOC 检查接口占位。
- [ ] P2 实现自动游玩路径生成。

验收标准：

- QA 报告可导出。
- Headless Test 能消费 QA 生成的路径。

## 7. Phase 5：Build Pipeline

目标：能生成独立、确定性、可审计的发布包。

### 7.1 AstraAssetCooker

- [ ] P0 创建 `AstraAssetCooker` executable。
- [ ] P1 实现读取 `.asset.yaml` sidecar。
- [ ] P1 实现生成 AssetRegistry。
- [ ] P1 实现依赖收集。
- [ ] P1 实现脚本编译。
- [ ] P1 实现图片 cook。
- [ ] P1 实现音频 cook。
- [ ] P1 实现字体图集生成。
- [ ] P1 实现本地化表生成。
- [ ] P2 实现 external asset registry 校验。
- [ ] P2 实现 mount-only policy，默认不复制外部原始资产。

验收标准：

- Cooked Content 不依赖源资产路径。
- Cook 从 sidecar 和文本源生成 registry，不从 ad hoc binary path 推断语义。
- 缺失依赖会阻塞 cook。
- Mount-only 项目不会把外部原始资产写入 cooked output。

### 7.2 AstraBuildTool

- [ ] P1 创建 `AstraBuildTool` executable。
- [ ] P1 读取项目构建配置。
- [ ] P1 调用 CMake 构建目标。
- [ ] P1 输出构建日志。
- [ ] P2 支持多平台构建配置。

验收标准：

- 可一条命令构建 `AstraGame` 和项目 Runtime 模块。
- 构建失败原因可追踪。

### 7.3 AstraPackageTool

- [ ] P1 创建 `AstraPackageTool` executable。
- [ ] P1 打包 Cooked Content。
- [ ] P1 生成 package manifest。
- [ ] P1 生成 AI Content Audit。
- [ ] P1 支持 Deterministic Build。
- [ ] P2 支持 Hybrid Build。
- [ ] P2 支持 patch 包。

验收标准：

- 发布包可离线运行。
- Deterministic Build 不包含运行时 LLM。

### 7.4 Release Gate

- [ ] P0 定义 Release Gate 配置。
- [ ] P1 检查 YAML 解析和 JSON Schema。
- [ ] P1 检查脚本编译状态。
- [ ] P1 检查 Story Graph 连接状态。
- [ ] P1 检查资产依赖。
- [ ] P1 检查缺失 sidecar。
- [ ] P1 检查重复 ID。
- [ ] P1 检查 AssetRegistry 与 sidecar 同步。
- [ ] P1 检查本地化 key。
- [ ] P1 检查未审核 AI 内容。
- [ ] P1 检查 Runtime AI 与发布模式是否冲突。
- [ ] P1 检查 PluginDescriptor schema、模块 ABI version、依赖闭包、权限声明和 runtime packaging eligibility。
- [ ] P1 检查 external asset root 是否存在。
- [ ] P1 检查 mount-only 项目是否试图复制外部原始资产。
- [ ] P1 检查 compatibility module 配置和 external metadata。
- [ ] P2 检查文本溢出。
- [ ] P2 检查存档版本迁移。

验收标准：

- 未审核 AI 内容会阻塞 Deterministic Build。
- Gate 报告能指出阻塞项和修复入口。
- Gate 能阻止 invalid YAML、schema mismatch、missing sidecar、broken dependency。
- Gate 能阻止 ABI 不兼容模块、缺失动态模块依赖、未授权权限和 Editor/Developer/MCP debug 模块进入 runtime package。
- Gate 能阻止未授权 external asset copy。

## 8. Phase 6：Compatibility Modules

目标：支持外部 VN 项目的探测、只读挂载、资产解析、RuntimeCommand source 和现代化覆盖。

### 8.1 CompatibilityCore

- [ ] P0 创建 `Astra_CompatibilityCore` target。
- [ ] P0 定义 `ICompatibilityAdapter` extension point。
- [ ] P0 定义 `IForeignProjectProbe`。
- [ ] P0 定义 `IForeignProject`。
- [ ] P0 定义 `IForeignPackageMount`。
- [ ] P0 定义 `IForeignAssetResolver`。
- [ ] P0 定义 `IForeignScriptAdapter`。
- [ ] P0 定义 `IRuntimeCommandSource`。
- [ ] P1 定义 `ISaveExtensionStateProvider`。
- [ ] P1 定义兼容诊断数据结构。

验收标准：

- CompatibilityCore 依赖 ModuleRuntime、ExtensionRegistry、Runtime Services、AssetRegistry 和 VFS，不依赖 Editor。
- Compatibility module 可由动态模块通过 ExtensionRegistry 注册。
- Compatibility module 只能通过 RuntimeCommandSource、Runtime Services extension、Runtime ECS system pack 或 SaveService extension state 参与运行时。
- Compatibility module 不直接访问 EnTT、Renderer2D、AudioCore 或 PlatformSDL3。

### 8.2 Mount-Only Package / Asset Resolver

- [ ] P1 实现外部项目 probe。
- [ ] P1 实现普通目录外部项目只读 mount。
- [ ] P1 实现外部资产路径规范化。
- [ ] P1 实现 external asset metadata。
- [ ] P1 实现 `foreign-director` AssetId scheme。
- [ ] P1 实现 `foreign-renpy` AssetId scheme。
- [ ] P1 实现 mount-only 缺失路径诊断。
- [ ] P2 实现 RPA mount。
- [ ] P2 实现 XP3 mount。
- [ ] P2 实现 Director `.dir/.dxr/.cxt` mount prototype。
- [ ] P2 实现编码诊断。

验收标准：

- 能列出外部项目图片和音频 external refs。
- 缺失资产能进入诊断报告。
- Mount-only 默认不复制、不转换、不重打包外部原始资产。

### 8.3 Mock Compatibility Module

- [ ] P1 创建 mock compatibility module。
- [ ] P1 通过 RuntimeCommandSource 显示背景。
- [ ] P1 通过 RuntimeCommandSource 显示对白。
- [ ] P1 通过 RuntimeCommandSource 发出音频请求。
- [ ] P1 实现 SaveService extension state 保存和恢复。
- [ ] P1 实现 RuntimeCommand log 测试。

验收标准：

- Mock compatibility module 可通过 ModuleManager 加载并注册扩展。
- Headless Test 能运行 mock module，不初始化 Renderer2D 或 AudioCore。
- Save/Load 可恢复 compatibility extension state。

### 8.4 Director Compatibility 原型

- [ ] P2 创建 `DirectorCompatibility` 插件骨架。
- [ ] P2 探测 Director / Projector 结构。
- [ ] P2 识别 `.dir`、`.dxr`、`.cxt`、`.x32`。
- [ ] P2 实现 read-only package mount prototype。
- [ ] P2 实现 cast/member external asset index prototype。
- [ ] P2 实现 score/timeline 诊断摘要。
- [ ] P3 实现 Lingo/score emulation prototype。

验收标准：

- 能识别 Director 风格老游戏目录。
- 能生成 external asset refs 和诊断报告。
- 不默认解密、破解或绕过受保护商业包。

### 8.5 CompatibilityEditor

- [ ] P2 创建 Compatibility Inspector。
- [ ] P2 展示项目识别结果。
- [ ] P2 展示启用的 compatibility modules。
- [ ] P2 展示 mount-only 状态。
- [ ] P2 展示包挂载状态。
- [ ] P2 展示 external asset refs。
- [ ] P2 展示 SaveService extension state 摘要。
- [ ] P2 展示现代化覆盖配置。
- [ ] P2 导出诊断报告。

验收标准：

- 外部项目问题可通过 Inspector 定位。

### 8.6 Compatibility Test Fixtures

- [ ] P1 创建 `CompatibilityFixtures/Mock/minimal_service_port`。
- [ ] P2 创建 `CompatibilityFixtures/Director/minimal_cast_mount`。
- [ ] P2 创建 `CompatibilityFixtures/RenPy/minimal_dialogue`。
- [ ] P2 创建 KiriKiri fixture 占位。
- [ ] P2 创建 NScripter fixture 占位。

验收标准：

- 每个 fixture 有 golden RuntimeCommand log 或 diagnostics。

## 9. Phase 7：Runtime AI 与高级插件

目标：在不破坏确定性发布的前提下支持受约束运行时 AI 和高级表现插件。

### 9.1 Runtime AI

- [ ] P2 定义 Runtime AI policy。
- [ ] P2 定义 Flavor AI 模式。
- [ ] P2 定义 Reactive AI 模式。
- [ ] P2 实现 runtime prompt snapshot。
- [ ] P2 实现 output snapshot。
- [ ] P2 实现 fallback 内容。
- [ ] P2 实现禁用 Runtime AI 后的回放策略。
- [ ] P3 研究 Branch AI。
- [ ] P3 研究 Experimental AI Director。

验收标准：

- Runtime AI 只能在项目策略允许时启用。
- 运行时生成内容可保存、回放、禁用和 fallback。

### 9.2 TTS Provider

- [ ] P2 定义 TTS Provider 接口。
- [ ] P2 实现 TTS 预览缓存 key。
- [ ] P2 支持 speaker 和 emotion 参数。
- [ ] P2 将 TTS 输出作为 Voice Asset 引用。
- [ ] P3 支持多 Provider 切换。

验收标准：

- 相同 text、speaker、emotion、model 命中同一缓存。
- TTS 预览内容默认不进入发布包，除非人工确认。

### 9.3 Live2D / Spine

- [ ] P3 定义动态角色表现插件接口。
- [ ] P3 创建 Live2D 插件骨架。
- [ ] P3 创建 Spine 插件骨架。
- [ ] P3 将动态角色接入 StageService。
- [ ] P3 支持表情和动作切换。

验收标准：

- 插件可选安装。
- 没有 Live2D / Spine 插件时基础 VN Demo 不受影响。

## 10. 横向任务

### 10.1 文档维护

- [ ] P0 在每个新增模块下添加 `README.md` 说明职责和依赖。
- [ ] P1 更新 `docs/design` 中与实际实现不一致的部分。
- [ ] P1 将重大技术选择记录为 ADR。
- [ ] P2 增加开发者上手文档。

### 10.2 测试矩阵

- [ ] P0 建立 `Engine/Developer/AutomationTest`。
- [ ] P1 Core 单元测试。
- [ ] P1 VFS 单元测试。
- [ ] P1 AssetId 单元测试。
- [ ] P1 YAML source schema 单元测试。
- [ ] P1 Asset sidecar validation 单元测试。
- [ ] P1 PluginDescriptor schema 单元测试。
- [ ] P1 ModuleManager discovery / dependency / load phase 单元测试。
- [ ] P1 Dynamic module ABI smoke test。
- [ ] P1 ExtensionRegistry 注册和权限诊断测试。
- [ ] P1 VN Property System schema generation 测试。
- [ ] P1 DSL parser 单元测试。
- [ ] P1 RuntimeCommandExecutor 集成测试。
- [ ] P1 Astra Runtime session lifecycle 单元测试。
- [ ] P1 Runtime extension API 集成测试。
- [ ] P1 Runtime ECS World 单元测试。
- [ ] P1 Runtime ECS Schedule 集成测试。
- [ ] P1 Headless VN Test。
- [ ] P2 TextCore golden test。
- [ ] P2 AI Policy test。
- [ ] P2 MCP trusted direct write test。
- [ ] P2 MCP Operation Log test。
- [ ] P2 Compatibility RuntimeCommand source golden playback test。
- [ ] P2 Mount-only package policy test。

### 10.3 示例内容

- [ ] P1 最小 VN Demo。
- [ ] P1 多语言文本样例。
- [ ] P1 Asset sidecar 样例。
- [ ] P1 Character/Lore/Story/Localization YAML 样例。
- [ ] P1 缺失资产诊断样例。
- [ ] P2 复杂分支样例。
- [ ] P2 AI Review Queue 样例。
- [ ] P2 外部引擎兼容样例。

## 11. 当前建议启动顺序

1. 完成 Phase 0 工程骨架。
2. 完成 Core、ApplicationCore、PlatformSDL3。
3. 完成 ModuleRuntime、ExtensionRegistry、PluginDescriptor schema 和 VN Property System 最小版本。
4. 确定 Renderer2D 后端并完成最小绘制。
5. 完成 AssetId、VFS 普通目录 mount、AssetRegistry。
6. 完成 Text-First schema、asset sidecar、registry generation。
7. 完成 RuntimeCommand、VNRuntimeServices 和 Runtime extension API。
8. 完成 Astra Runtime session。
9. 完成最小 Astra DSL parser。
10. 跑通 `AstraGame --project Projects/Samples/MinimalVN`。
11. 再进入 Editor、AI Suggestion Layer、MCP Integration 和 Compatibility Modules。

# 路线图

## 1. 路线原则

路线图以“先跑通确定性 VN，再引入 AI 协作，再扩展兼容层”为主线。

优先级规则：

- 先稳定 Runtime Services，再做编辑器高级功能。
- Runtime Services 内部先稳定 ECS World 和固定 Schedule，再扩展复杂舞台、动画和 Runtime MCP / Generation。
- 源项目数据先稳定 Text-First YAML + JSON Schema，再做高级编辑器功能。
- MCP 先统一为 Agent 能力协议层；Phase 3 先落地 Editor MCP Host，Phase 7 再落地 Runtime MCP Host 和 Runtime Generation。
- 动态模块作为默认扩展模型，源码级模块只用于核心、实验性底层能力或未稳定 ABI 的内部代码。
- UE 级可定制化限定在视觉小说领域，不扩张为通用 3D / Gameplay 引擎。
- 先确定性发布，再做运行时 MCP / Generation。
- 先最小可玩 Demo，再做完整工具链。
- 每阶段必须有可验收产物。

## 2. Phase 0：仓库与设计基线

目标：形成可协作的项目骨架。

交付物：

- `docs/design` 设计文档。
- Text-First 源数据规范。
- 统一 MCP / Agent 能力层设计。
- Dynamic Module / ExtensionRegistry / VN Property System 设计。
- 顶层 CMake 项目。
- vcpkg manifest。
- 基础目录结构。
- 编码规范和模块命名约定。

验收标准：

- 新开发者能通过 README 理解系统目标和模块边界。
- CMake 能配置空项目。
- 文档明确 Runtime 不依赖 Editor。
- 文档明确 canonical source data 是 YAML + JSON Schema。
- 文档明确 Agent Audit、Operation Log 和 Generation Audit 的拆分。
- 文档明确动态模块优先、C ABI 边界和 VN Property System。

## 3. Phase 1：基础 Runtime

目标：能通过 Astra Runtime 运行一个确定性 Astra VN Demo。

实现：

- Core。
- SDL3 ApplicationCore。
- PlatformSDL3。
- Renderer2D。
- AudioCore。
- TextCore。
- AssetCore。
- Text-First sidecar asset metadata。
- ModuleRuntime。
- ExtensionRegistry。
- VN Property System。
- VNRuntimeServices。
- Runtime ECS World。
- Astra DSL 最小解析。
- Astra Runtime。
- SaveGame。
- AstraGame。

验收标准：

- Demo 能显示背景、立绘和对白。
- `.asset.yaml` sidecar 能生成 AssetRegistry。
- 能播放 BGM 和 SFX。
- 能处理选择分支。
- 能保存和读取。
- ECS schedule 可在 headless 模式下运行。
- 示例动态模块可通过 ModuleManager 加载并注册一个测试扩展。
- 插件定义的 VN property type 可生成 schema。
- Headless Test 能跑通一条剧情路径。

## 4. Phase 2：Editor 基础

目标：能编辑和预览 VN 项目。

实现：

- AstraEditor。
- Project Browser。
- Content Browser。
- Asset Detail Panel。
- Text source schema validation。
- Script Editor。
- Scene Preview。
- Play In Editor。
- Output Log。

验收标准：

- 能创建项目。
- 能导入图片和音频。
- 能生成并编辑 `.asset.yaml` sidecar。
- 能编辑最小脚本。
- 能从编辑器启动 PIE。
- 编辑器预览与 AstraGame 使用同一 Runtime Services。

## 5. Phase 3：AI Suggestion Layer

目标：AI 能辅助创作，但不能直接覆盖正式内容。

实现：

- AgentWorkbench。
- Prompt Studio。
- Boundary Manager。
- Review Queue。
- Diff/Patch。
- Agent Audit。
- AI Provider 插件接口。
- Editor MCP Host。
- MCPCore 与 Operation / Generation Audit 契约。

验收标准：

- AI 生成对白建议进入 Review Queue。
- 接受、编辑、拒绝都有审计记录。
- AI 不能修改 locked Canon Lore。
- 发布前能生成 AI Content Audit。
- 无审核内容时 Deterministic Build 通过 release gate。
- Editor MCP trusted direct write 能修改文本源文件并记录 Operation Log。
- Editor MCP resources 能读取项目上下文、资产元数据、脚本、设定和构建状态。
- Agent Audit 能区分 tool side effect 和内容生成来源。

## 6. Phase 4：完整 VN Authoring

目标：形成完整 VN 制作套件。

实现：

- StoryGraphEditor。
- CharacterEditor。
- LoreEditor。
- SceneEditor。
- LocalizationEditor。
- YAML + JSON Schema authoring tools。
- QA / Eval Lab。
- 文本溢出检测。
- 分支覆盖检测。

验收标准：

- 能从 Story Graph 生成可运行脚本或执行计划。
- 能维护角色卡和 Canon Lore。
- 能生成本地化表并检测缺失 key。
- 能校验角色、设定、剧情图、本地化和 Review Queue YAML schema。
- QA 工具能报告死分支和未使用资产。

## 7. Phase 5：Build Pipeline

目标：能打包发布。

实现：

- AstraAssetCooker。
- AstraBuildTool。
- AstraPackageTool。
- Runtime AssetRegistry。
- Sidecar source validation。
- AI Release Gate。
- Deterministic Build。

验收标准：

- 能生成独立运行包。
- Cooked Content 不依赖编辑器源资产。
- Cook 从 sidecar 和文本源生成 registry，不依赖 ad hoc binary paths。
- 未审核 AI 内容会阻塞 Deterministic Build。
- 发布包能离线运行。
- 构建产物有 manifest 和审计报告。

## 8. Phase 6：Compatibility Modules

目标：先支持外部引擎包的 mount-only 动态兼容模块和现有扩展机制集成。

实现：

- VFS 扩展。
- CompatibilityCore。
- ForeignProject。
- ForeignAssetResolver。
- CompatibilityAdapter extension。
- ForeignPackageMountProvider。
- ForeignScriptAdapter / RuntimeCommandSource。
- SaveService extension state。
- CompatibilityEditor。
- Mount-Only Compatibility Mode。
- Director compatibility prototype。

验收标准：

- 能探测至少一种外部 VN 项目。
- 能只读挂载外部包并解析图片、音频。
- Mock compatibility module 能通过 RuntimeCommand source 或 Runtime Services extension 驱动背景、对白和音频请求。
- Cook/package 默认拒绝复制外部原始资产。
- 能在 Compatibility Inspector 中查看诊断。

## 9. Phase 7：Runtime MCP / Generation 与高级插件

目标：支持更丰富的互动与扩展。

实现：

- Runtime MCP Host。
- Runtime Generation Orchestrator。
- Local LLM Provider。
- Image Generation Provider。
- TTS Provider。
- Agent Audit packaged runtime mode。
- Live2D。
- Spine。
- 外部引擎高级兼容插件。

验收标准：

- Runtime MCP Host 和 Runtime Generation 只能在项目策略允许时启用。
- 运行时生成内容可保存、回放、禁用和 fallback。
- Runtime 侧 generation / provider / audit 模块都通过 packaged eligibility 校验。
- TTS 预览缓存可复用。
- Live2D / Spine 作为插件接入 Renderer2D 或 StageService。

## 10. 主要风险

### 10.1 文本系统复杂度

风险：中日英混排、ruby、emoji、富文本、逐字显示叠加后复杂度高。

缓解：

- TextCore 独立成模块。
- 尽早引入 FreeType + HarfBuzz。
- 建立文本渲染 golden test。

### 10.2 AI 功能侵入核心

风险：AI 逻辑直接污染 Runtime 和项目格式，后续难以关闭或发布确定性版本。

缓解：

- Runtime Generation、Provider 和 Audit 默认模块化。
- AI 修改必须走 Patch 和 Review Queue。
- Release Gate 默认阻止未审核内容。

### 10.3 ECS 边界泄露

风险：EnTT 类型泄露到 Editor、Compatibility 插件或 VN DSL 后，后续替换 ECS 实现或稳定公开 API 的成本会显著上升。

缓解：

- Runtime Services facade 返回引擎自有 DTO 或快照。
- RuntimeCommand 和 Runtime Services facade 保持稳定，兼容模块不直接访问 EnTT。
- ADR 0004 明确 EnTT 是内部实现细节。

### 10.4 MCP 权限边界风险

风险：MCP 受信直写绕过 Review Queue，可能产生难以审查的项目变更。

缓解：

- Editor MCP 默认禁用，需要显式 trusted session。
- Runtime MCP 不允许 project_write。
- 每个 mutating tool 写 Operation Log，运行时生成写 Generation Audit Log。
- 路径限制在 workspace/project 或 runtime-safe DTO 范围内。
- Release Gate 校验 YAML、schema、依赖和构建状态。

### 10.5 Text-First schema 演进风险

风险：YAML schema 频繁变化会影响 AI、MCP、Editor 和 Cook 工具。

缓解：

- Schema 文件版本化。
- Cook 和 Release Gate 先支持最小必需 schema。
- 大规模 schema 变更通过 ADR 或 migration 工具处理。

### 10.6 编辑器范围膨胀

风险：过早构建大型编辑器导致 Runtime 不稳定。

缓解：

- Phase 1 优先 Headless 和 AstraGame。
- Editor 只使用已稳定 Runtime Services。
- 每个编辑器工具都要有对应 CLI 或自动化测试路径。

### 10.7 外部兼容层成本过高

风险：追求高兼容度会拖慢核心引擎。

缓解：

- 第一阶段只做 probe、只读 mount、external asset refs、RuntimeCommand source 和 diagnostics。
- 每个 compatibility module 独立维护测试 fixture。
- 复杂 timeline / score / VM 适配延后。

### 10.8 动态模块 ABI 风险

风险：动态模块作为默认扩展模型后，ABI 边界设计不当会导致核心接口演进成本变高，或插件直接依赖内部 C++ 实现细节。

缓解：

- ABI 边界采用窄 C ABI、opaque handle、POD descriptor 和 diagnostics sink。
- 不跨 ABI 暴露 STL、EnTT、Renderer2D、AudioCore、PlatformSDL3 或 Editor 内部对象。
- 公开扩展点必须通过 ExtensionRegistry、Runtime Services facade、DTO 和 VN Property System。
- 源码级模块只保留给核心和实验性内部能力，不作为项目级扩展默认路径。
- Release Gate 校验插件 descriptor、ABI version、权限、依赖闭包和 packaged runtime eligibility。

## 11. Backlog

近期候选任务：

- 建立 CMake / vcpkg 基础工程。
- 定义 Core 的错误、日志、路径和模块系统。
- 选择 Renderer2D 后端。
- 定义 AssetId、AssetMetadata、AssetRegistry 文件格式。
- 定义 `.asset.yaml` sidecar 和 JSON Schema。
- 定义 MCPCore、Editor MCP Host、Runtime MCP Host、resources、tools、prompts 和审计契约。
- 定义 Runtime Generation Orchestrator、Provider 接口和 Agent Audit 事件模型。
- 定义 ModuleManager、AstraModule C ABI、ExtensionRegistry 和 PluginDescriptor schema。
- 定义 VN Property System 的 TypeId、PropertyId、schema generation 和 editor metadata。
- 定义最小 Astra DSL AST。
- 定义 RuntimeCommand 和 RuntimeCommandExecutor。
- 定义 Astra Runtime、RuntimeCommand 和 Runtime Services facade。
- 定义 Runtime ECS World、组件、资源和固定 Schedule。
- 建立最小 Demo 项目。
- 建立 Headless Test。

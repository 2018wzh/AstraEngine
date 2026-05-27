# 术语表

## A

### Agent Workbench

编辑器中用于运行、调试、配置 AI Agent 的工作区。普通 AI 建议不能直接写入正式项目内容，必须通过 Boundary Manager 和 Review Queue。受信 MCP 会话是独立的 Developer 工具链能力，应按 MCP 权限模型和 Operation Log 处理。

### AI Provider

提供 AI 能力的插件或服务适配器，例如云模型、本地 LLM、图像生成、TTS。Provider 需要声明网络、运行时可用性和权限。

### AssetId

引擎内部统一资产标识。示例：

```text
native:/Characters/Alice
foreign-renpy:/images/alice happy.png
virtual:/current/character/alice
```

### AssetRegistry

资产元数据和依赖索引。编辑器用它查询、验证和 cook 资产，运行时用 cook 后 registry 加载资源。

### Astra Runtime

Astra 原生运行时主线，负责执行 Astra DSL、Story Graph、RuntimeCommand、变量、场景状态和存档快照。兼容模块不替代 Astra Runtime，只通过扩展点参与。

### AstraModule

动态模块暴露给引擎的稳定 C ABI entrypoint。它通过 host API 和 ExtensionRegistry 注册能力，不跨 ABI 暴露 STL、EnTT、Renderer/Audio 原生句柄或 Editor 内部对象。

## B

### Boundary Manager

AI 权限和策略执行器。它判断某次 AI 操作是否允许、是否需要审核、能修改哪些目标、哪些目标被锁定。

## C

### Canon Lock

正式设定锁。被锁定的 Canon Lore 可被 AI 引用，但不能被 AI 直接修改。

### Canonical Project

项目正式内容集合。只有人类接受或编辑后的内容才能进入该集合。

### Capability

动态模块声明自己希望注册的能力，例如 compatibility adapter、runtime command source、AI provider、MCP tool、asset validator、cook processor 或 editor panel。Capability 必须与权限声明和 ExtensionRegistry 注册行为一致。

### Compatibility Layer

外部 VN 引擎兼容层，用于只读挂载、解析、诊断和现代化 Ren'Py、KiriKiri、NScripter、Director 等引擎的项目或包。它通过动态模块、VFS、AssetRegistry、RuntimeCommand、Runtime Services extension、SaveService extension state、Editor/MCP/Cook 扩展实现，不以 Import 为目标。

### Compatibility Module

外部引擎兼容动态模块，例如 DirectorCompatibility、RenPyCompatibility 或 KiriKiriCompatibility。它可以注册项目探测、包挂载、资产解析、脚本/timeline 适配、RuntimeCommand source、现代化覆盖、SaveService extension state 和诊断工具。

### Cook

将源资产转换为运行时可高效加载格式的过程，包括脚本编译、纹理压缩、音频转码、字体图集生成、依赖收集等。

### Cooked Content

Cook 之后进入发布包的运行时内容。

### CommandBuffer

Runtime ECS 中用于延迟写入 World 的命令队列。RuntimeCommand、Astra Runtime、兼容模块和受约束 Runtime AI 应通过 CommandBuffer 或服务 facade 进入 ECS，而不是直接暴露 EnTT registry。

### Component

挂在 Entity 上的纯数据，例如 Transform2D、Sprite、Dialogue、Transition 或 AudioRequest。Component 不持有全局服务。

## D

### Deterministic Build

确定性发布模式。只包含已审核固定内容，不包含运行时 LLM，适合商业发布。

### Diff / Patch

AI 和工具对项目提出变更的标准格式。Patch 必须可审查、可回滚、可检查 stale 状态。

### Dynamic Module

AstraEngine 默认项目级扩展形态。动态模块由 ModuleManager 加载，通过 AstraModule C ABI 和 ExtensionRegistry 注册能力，可用于 Runtime、Editor、Developer、MCP、Asset Pipeline、AI Provider 和 Compatibility 扩展。

## E

### ECS

Entity Component System。AstraEngine 在 Runtime Services 内部使用 EnTT 实现 Bevy 风格 ECS，用于组织运行时实体、组件、资源和系统调度。ECS 是内部实现细节，不直接暴露给 VN DSL、Editor、Compatibility 插件或 AI 插件。

### Editor/Game 分离

编辑器可以依赖运行时，运行时不能依赖编辑器。发布包不包含编辑器模块。

### Entity

Runtime ECS World 中的运行时对象，例如背景、立绘、对白显示、短生命周期特效或音频请求。公开接口不传递 EnTT entity。

### Engine Component

Compatibility Module 内部的组合单元，例如包挂载、资产解析、脚本 VM、timeline/score 或存档适配组件。它不是 Runtime Services 的替代品，必须通过扩展点接入。

### ExtensionRegistry

动态模块注册扩展点的中心注册表。它接收 RuntimeCommand source、CompatibilityAdapter、VFS mount provider、ForeignAssetResolver、Service extension、ECS system pack、脚本函数、Story Graph 节点、AssetValidator、CookProcessor、Editor panel、MCP provider 和 AI provider 等注册。

## F

### ForeignProject

兼容层表示的外部 VN 项目，负责提供只读文件挂载、资产解析、现代化配置和兼容诊断。

## H

### Headless Test

无窗口自动化运行模式，用于跑剧情路径、分支覆盖、脚本验证和 CI 测试。

## L

### Load Phase

动态模块加载阶段，例如 project_load、asset_registry、compatibility_probe、runtime_startup、editor_startup、mcp_startup 和 cook_startup。ModuleManager 使用它决定注册顺序和依赖约束。

## M

### MCP

Model Context Protocol。AstraEngine 将 MCP 作为 Editor/Developer 插件能力，用于向外部 AI 工具暴露项目资源、验证工具、构建工具和受控写入入口。默认不进入 packaged runtime。

### MCP Prompt

MCP server 暴露的可复用任务提示，例如对白润色、设定一致性检查、角色 OOC 检查、本地化草稿和 QA 路径分析。Prompt 只描述任务模板，不绕过项目权限。

### MCP Resource

MCP server 暴露的只读项目视图，例如 `astra://project/manifest`、`astra://assets/registry`、`astra://characters/{id}` 和 `astra://review-queue`。Resource 不暴露明文密钥或 ECS/EnTT 内部状态。

### MCP Tool

MCP server 暴露的可执行能力，例如 `script.validate`、`asset.write_sidecar`、`test.run_headless`、`build.cook` 和 `release.run_gate`。会修改项目的 tool 必须在 trusted session 中运行并记录 Operation Log。

### Mount-Only Compatibility Mode

兼容模块默认模式。它只读访问用户本地原游戏目录，不复制、不转换、不重打包外部原始资产。

### ModuleManager

动态模块管理器，负责发现插件、校验 PluginDescriptor、解析依赖、校验版本和权限、加载动态库、执行生命周期、卸载模块并输出诊断。

## O

### Operation Log

MCP mutating tool 的追加式操作日志。记录 session、actor、tool、输入摘要、受影响路径、校验结果和输出摘要，用于审计、回滚定位和 Release Gate 诊断。

## P

### Play In Editor

编辑器内运行游戏的模式。PIE 应使用同一套 Runtime Services，而不是复制独立运行逻辑。

### Permission

动态模块请求的受控能力，例如 project write、external mount read、network、MCP tool registration、runtime packaged、editor UI extension 或 cook output write。未声明权限的模块不能注册对应能力。

### PluginDescriptor

插件的文本源描述文件，通常为 `*.plugin.yaml`。它记录插件 ID、版本、Astra API 版本范围、模块类型、entrypoint、load phase、依赖、capability、permission 和平台过滤。

### Provenance / Audit Log

来源和审计日志。记录 AI 输出、上下文 hash、模型、作者动作和最终内容之间的关系。

## R

### Release Gate

发布前质量门禁。用于阻止未审核 AI 内容、缺失资产、脚本错误、本地化缺失、运行时 AI 策略冲突等问题进入发布包。

### Review Queue

AI 输出和工具建议的审核队列。创作者可以接受、编辑、拒绝、延后或拆分变更。

### Resource

Runtime ECS 中的全局或帧级状态，例如输入、存档、对话历史、音量总线、运行时配置和资产注册表访问。

### Runtime AI

发布后运行时动态 AI。默认关闭，只能在项目策略和发布模式允许时启用。

### RuntimeCommand

Astra DSL、Story Graph、Astra Runtime 内部 AI hook 和 compatibility adapter 到 Runtime Services 的命令协议。

### RuntimeCommand Source

可在运行时产生 RuntimeCommand 的扩展点。Astra DSL、Story Graph、Runtime AI 和兼容模块都可以作为 RuntimeCommand source。

### Runtime Extension API

动态模块参与运行时的公开扩展 API，包括 RuntimeCommand source、Runtime Services extension、受控 Runtime ECS system pack 和 SaveService extension state provider。

### Runtime Services

稳定运行时服务层，包括 Stage、Dialogue、Choice、Audio、Asset、Input、Save、Localization。Astra Runtime、兼容模块、Runtime AI、Editor Preview 和 Headless Test 共享该层。内部使用 Runtime ECS World 组织状态和系统调度。

### Runtime ECS World

Runtime Services 拥有的 ECS 容器，包含 Entity、Component 和 Resource。它负责运行固定 Schedule，但不替代 RuntimeCommand 或服务 facade。

## S

### Schedule

Runtime ECS 的固定系统执行顺序。第一阶段顺序为 Input、Script、CommandApply、Animation、Audio、RenderExtract、SaveSnapshot、Cleanup。

### SaveService Extension State

动态模块保存运行时扩展状态的机制。状态必须由 VN Property System 描述 schema 和迁移策略，并通过 SaveService 进入统一快照。

### Sidecar Asset Metadata

二进制资源旁的同名 `.asset.yaml` 元数据文件，是图片、音频、字体、Live2D、Spine 等资源语义的 canonical source。AssetRegistry 由 sidecar 扫描生成，不作为主要人工或 AI 编辑源。

### Story Graph

剧情图结构，包含 Scene、Dialogue、Choice、Condition、Agent Generation、Cutscene、Ending、Subroutine 等节点。

### System

读取或写入 ECS Component 和 Resource 的运行时逻辑单元，例如动画推进、音频请求消费、渲染数据抽取和短生命周期实体清理。

## T

### Text Source Schema

针对 YAML 源数据解析结果的 JSON Schema。它定义必填字段、类型、稳定 ID、AI 可编辑字段、工具生成字段和只读字段。

### Text-First Source Data

项目源数据以文本为主的设计原则。角色、设定、剧情图、本地化、AI 策略、Review Queue、构建配置和资产 sidecar 使用 YAML + JSON Schema；Cooked Content 可以是二进制优化产物。

### Trusted Session

用户显式启动并授权的 MCP 会话。会话内 mutating tools 可在 workspace/project 边界内直接写入文本源文件，但必须记录 Operation Log，并拒绝未授权外部路径、明文 API key 和 ECS 内部状态访问。

## V

### VFS

虚拟文件系统。用于统一读取普通目录、ZIP、PAK、XP3、RPA、NSA、补丁包和外部包。

### VN Property System

AstraEngine 的视觉小说专用属性和类型描述系统。它提供稳定 TypeId、PropertyId、enum metadata、属性描述、默认值、编辑器元数据、AI 可编辑标记、schema 生成、序列化钩子和插件配置校验，用于替代完整 UObject 体系。

## W

### World

Runtime ECS 的实体、组件和资源容器。AstraEngine 使用 EnTT 实现 World，但 EnTT 类型不进入公开 API。

## Y

### YAML Source

人类、AI 和 MCP 共同编辑的项目源数据格式。YAML 文件可以包含注释，校验时先解析为数据节点，再由 JSON Schema 验证。

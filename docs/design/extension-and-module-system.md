# 扩展与动态模块系统设计

## 1. 目标

AstraEngine 只专注视觉小说领域，但在该领域内提供接近 UE 的可定制化和扩展性。扩展系统应覆盖运行时、编辑器、资产管线、AI、MCP 和兼容模块，同时避免把内部 C++、EnTT、Renderer2D、AudioCore 或 Editor 对象泄露给插件。

核心目标：

- 动态模块是默认项目级扩展方式。
- 源码级模块只用于引擎核心、实验性底层能力或尚未稳定 ABI 的内部代码。
- 插件作者通过稳定 C ABI 和 C++ SDK 注册扩展点。
- 视觉小说领域的对象、配置和编辑器面板通过 VN Property System 统一描述。
- 发布包只包含明确启用且 runtime-safe 的模块。

## 2. 非目标

- 不实现通用 3D 游戏引擎。
- 不引入完整 UE UObject、UHT、Actor/Gameplay 框架或 GC 对象模型。
- 不允许插件直接访问 EnTT registry、native renderer handle、native audio handle 或 editor UI object。
- 不承诺 v1 支持任意运行中二进制热替换。
- 不把动态模块权限系统作为操作系统级沙箱；它是引擎级能力边界和 Release Gate 校验机制。

## 3. 模块布局

```text
Plugins
├── OpenAIProvider
│   ├── OpenAIProvider.plugin.yaml
│   ├── Bin
│   │   └── win64/OpenAIProviderRuntime.dll
│   └── Content
├── DirectorCompatibility
│   ├── DirectorCompatibility.plugin.yaml
│   └── Bin
└── Live2D
    ├── Live2D.plugin.yaml
    └── Bin
```

插件描述文件是 canonical source，可由人类、AI 和 MCP 编辑，并通过 JSON Schema 校验。

```yaml
id: astra.plugin.director_compatibility
display_name: Director Compatibility
version: 0.1.0
astra_api: ">=0.1 <0.2"
modules:
  - id: director_compat.runtime
    type: runtime
    entrypoint: Bin/win64/DirectorCompatibility.dll
    load_phase: asset_registry
    capabilities:
      - compatibility_adapter
      - runtime_command_source
      - vfs_mount_provider
      - external_asset_resolver
    permissions:
      filesystem:
        project_read: true
        external_mount_read: true
        project_write: false
      network: false
    platforms:
      - windows-x64
dependencies: []
```

## 4. ModuleManager

`ModuleManager` 负责动态模块生命周期：

```text
Discover
  -> ValidateDescriptor
  -> ResolveDependencies
  -> CheckVersion
  -> CheckPermissions
  -> LoadBinary
  -> Initialize
  -> RegisterExtensions
  -> Activate
  -> Deactivate
  -> Shutdown
  -> Unload
```

职责：

- 扫描 Engine、Project 和 User 插件目录。
- 校验 `PluginDescriptor` schema。
- 解析依赖和 load phase。
- 校验 `astra_api`、插件版本、平台和目标包类型。
- 加载动态库并查找 `AstraModule` entrypoint。
- 收集模块诊断、能力和权限。
- 在 Release Gate 中报告不兼容、缺失依赖或错误权限。

## 5. AstraModule ABI

动态模块通过稳定 C ABI 进入引擎。

```cpp
extern "C" ASTRA_MODULE_EXPORT AstraModuleResult astra_module_main(
    const AstraModuleHostApi* host,
    AstraModuleApi* out_module);
```

ABI 规则：

- ABI 边界只使用固定宽度整数、C 字符串、opaque handle、函数指针和 POD descriptor。
- 不跨 ABI 传递 `std::string`、`std::vector`、`std::span`、`std::function`、exceptions、RTTI 类型或 C++ ownership。
- 不跨 ABI 暴露 EnTT、Renderer2D、AudioCore、PlatformSDL3 或 Editor 内部对象。
- 所有字符串编码使用 UTF-8。
- 错误通过显式 result code 和 diagnostic sink 返回。
- C++ SDK 只能是 ABI 的便利包装，不是稳定边界本身。

## 6. ExtensionRegistry

模块初始化后通过 `ExtensionRegistry` 注册能力。

第一阶段扩展点：

- `ServiceExtension`：注册 Stage、Dialogue、Audio、Input、Save、Localization 的可选能力扩展。
- `RuntimeCommandSource`：注册可在运行时产生 RuntimeCommand 的脚本、timeline 或兼容适配器。
- `CompatibilityAdapter`：注册外部项目 probe、mount-only 配置、诊断和现代化覆盖入口。
- `VfsMountProvider`：注册外部目录和归档格式读取。
- `ForeignAssetResolver`：注册 `foreign-*` AssetId 解析器。
- `SaveExtensionStateProvider`：注册可序列化扩展状态。
- `RuntimeEcsSystemPack`：注册受控系统包，但不暴露 EnTT 类型。
- `ScriptFunctionProvider`：扩展 Astra DSL 内置函数。
- `StoryGraphNodeProvider`：扩展 Story Graph 节点类型和验证规则。
- `AssetValidator`：扩展 sidecar、依赖、license、external asset 校验。
- `CookProcessor`：扩展图片、音频、字体、Live2D、Spine 或自定义 runtime asset cook。
- `EditorPanelProvider`：扩展面板、菜单、详情页、预览器和诊断视图。
- `McpProvider`：注册 MCP resources、tools 和 prompts。
- `AIProvider`：注册云端模型、本地模型、TTS 或图像生成提供者。

注册规则：

- 扩展点必须声明 capability。
- 需要写项目、访问外部路径、联网或进入 packaged runtime 的扩展必须声明 permission。
- Runtime 扩展不得依赖 Editor 扩展。
- MCP 和 Developer 扩展默认不进入 packaged runtime。

## 7. VN Property System

VN Property System 是 Astra 的轻量属性和类型描述系统，用于替代完整 UObject。

能力：

- 稳定 `TypeId`、`PropertyId` 和 enum metadata。
- 描述 scalar、string、localized text、asset ref、struct、array、map、enum 和 tagged union。
- 标记 `ai_editable`、`tool_generated`、`read_only`、`requires_review`。
- 生成 JSON Schema，支持 YAML 源数据校验。
- 驱动 Editor property panel、MCP field-level editing、diff、audit 和 serialization。
- 支持插件定义配置类型、资产 metadata 扩展、Story Graph 节点属性和 compatibility modernization settings。

示例：

```yaml
type_id: astra.vn.stage.transition
display_name: Transition
properties:
  - id: kind
    type: enum
    enum: astra.vn.transition_kind
    default: fade
    ai_editable: true
  - id: duration
    type: float
    default: 0.5
    min: 0.0
    max: 10.0
    ai_editable: true
```

## 8. 权限与安全

权限是模块能力边界和发布校验输入。

推荐权限：

- `filesystem.project_read`
- `filesystem.project_write`
- `filesystem.external_mount_read`
- `filesystem.external_mount_write`
- `network`
- `mcp.register_tools`
- `ai.provider`
- `runtime.packaged`
- `editor.extend_ui`
- `cook.write_output`
- `compat.mount_external`

规则：

- 未声明权限的模块不能注册对应扩展。
- 受信 MCP session 仍必须遵守模块权限和 workspace/project path boundary。
- 模块不得读取明文 API key；密钥访问必须通过 secret provider。
- 外部原游戏目录默认只读。
- Release Gate 阻止未授权权限、runtime 包含 authoring-only 模块或动态模块依赖缺失。

## 9. Load Phase

推荐 load phase：

- `core_startup`：仅引擎内置模块使用。
- `project_load`：读取项目配置和 schema 前后。
- `asset_registry`：注册 asset type、validator 和 sidecar 扩展。
- `compatibility_probe`：注册外部项目 probe、VFS mount provider 和 foreign asset resolver。
- `runtime_startup`：play session 启动前。
- `editor_startup`：编辑器 UI 扩展。
- `mcp_startup`：MCP resources、tools 和 prompts。
- `cook_startup`：cook processor 和 release gate 扩展。

## 10. 打包规则

发布包模块选择规则：

- 只打包启用的 runtime-safe 模块。
- `editor`、`developer`、`mcp_debug`、`authoring_only` 模块默认排除。
- `runtime.packaged` 权限必须显式声明。
- 模块依赖必须闭包完整。
- 模块 binary hash、descriptor hash 和 ABI version 进入 package manifest。
- Mount-only compatibility 项目默认不复制外部原始资产。

## 11. 测试策略

必须覆盖：

- Plugin descriptor schema validation。
- Module discovery、dependency sort、load phase order。
- ABI smoke test：加载示例模块、注册扩展、停用、卸载。
- Version mismatch diagnostic。
- Permission denial diagnostic。
- ExtensionRegistry 重复注册和缺失 capability。
- VN Property System schema generation 和 serialization。
- Dynamic compatibility module RuntimeCommand playback。
- Editor extension registration。
- MCP tool/resource registration。
- Cook processor registration。
- Packaged runtime 排除 Editor/Developer/MCP debug 模块。

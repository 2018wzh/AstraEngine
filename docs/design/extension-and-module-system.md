# 扩展与动态模块系统设计

状态：Phase 1 Foundation Implemented / Target Architecture

Phase 1 implementation note：当前工作树已实现 descriptor parsing/validation、dependency resolver、C ABI headers、`ModuleManager` lifecycle、`ServiceRegistry`、`ExtensionRegistry`、engine module provider registry、service resolve audit、engine module slot policy validation，以及 `AstraExampleFoundationPlugin` 的加载、注册、停用和卸载测试。`astra validate . --strict --json` 输出 Foundation module release-gate report，并包含模块 entrypoint binary existence 和 SHA-256 evidence。Plugin Wizard、hot reload 分层、provider-specific production contracts 和 Editor/AI/MCP provider 模板仍是后续阶段。

## 1. 目标

AstraEngine 使用动态模块作为默认扩展模型。模块系统必须支持通用 2D 引擎能力、VN Presentation、AI Provider、运行时 Intent、Editor 扩展和 Cook/Package 工具，同时保持 Core 干净、ABI 稳定和发布可审计。旧 VN 模拟器、现代化滤镜和 Compatibility Inspector 属于 Legacy expansion track，必须在稳定 native runtime API 之上接入。

## 2. 核心原则

- 动态模块通过稳定 C ABI 进入引擎。
- 模块不跨 ABI 传递 STL ownership、C++ Actor 指针、Renderer/Audio native handle 或 Editor widget。
- 模块通过 `ServiceRegistry` 获取服务，通过 `ExtensionRegistry` 注册能力。
- 所有能力必须声明 capability；文件、网络、AI、runtime packaging、MCP、外部挂载必须声明 permission。
- 热重载分层支持，不承诺任意运行中二进制替换。

## 3. 模块布局

```text
Plugins/Live2D
├─ Live2D.plugin.yaml
├─ Bin/win64/Live2D.dll
└─ Content

Plugins/BGICompat  (expansion track)
├─ BGICompat.plugin.yaml
└─ Bin/win64/BGICompat.dll
```

Descriptor 示例：

```yaml
id: astra.plugin.live2d
display_name: Live2D
version: 0.1.0
astra_api: ">=0.1 <0.2"
modules:
  - id: live2d.runtime
    type: runtime
    entrypoint: Bin/win64/Live2D.dll
    load_phase: runtime_startup
    capabilities:
      - actor_component_provider
      - state_machine_provider
      - script_api_provider
      - render_pass_provider
    permissions:
      runtime:
        packaged: true
```

Descriptor 完整字段：

```yaml
id: astra.plugin.example
display_name: Example Plugin
version: 0.1.0
astra_api: ">=0.1 <0.2"
description: Example runtime and editor plugin
author: project
license: internal
modules:
  - id: example.runtime
    type: runtime
    entrypoint: Bin/win64/ExampleRuntime.dll
    load_phase: runtime_startup
    dependencies:
      required: [astra.runtime]
      optional: [astra.media]
    capabilities:
      - service_provider
      - asset_importer
    permissions:
      project_read: true
      project_write: false
      network: false
      runtime:
        packaged: true
    exports:
      services: [example.service]
      extensions: [example.asset_importer]
      slots: []
release:
  packaged_eligible: true
  require_binary_hash: true
  allowed_profiles: [development, deterministic]
diagnostics:
  code_prefix: ASTRA_PLUGIN_EXAMPLE
```

Descriptor validation 必须检查：

- id/version/api range 格式。
- entrypoint path 不逃逸 plugin root。
- dependencies 不成环。
- capability 与 permission 匹配。
- module type 与 load phase 匹配。
- release profile 与 packaged eligibility 匹配。
- diagnostics code prefix 不冲突。

## 3.1 Plugin Authoring Experience

Plugin Wizard 必须提供以下模板：

- Runtime service plugin：注册 service、extension、diagnostics。
- Asset importer plugin：注册 `IAssetImporter` 和 import preset。
- Cook processor plugin：注册 `ICookProcessor` 和 package eligibility。
- Renderer/Text/Audio provider plugin：注册 EngineModuleSlot provider。
- Script runtime plugin：注册 `IScriptRuntimeProvider` 和 Script API provider。
- Editor panel plugin：注册 `IEditorPanelProvider`、菜单、命令和 panel layout metadata。
- AI/MCP plugin：注册 `IAIProvider`、`IMcpToolProvider`、audit sink 或 prompt pack。

每个模板必须生成：

- `*.plugin.yaml`
- public capability/permission 声明
- 最小 C ABI entry
- sample test
- manual page stub
- release gate checklist

Plugin wizard template descriptor 示例：

```yaml
id: astra.plugin_template.renderer2d
display_name: Renderer2D Provider
outputs:
  descriptor: ${plugin_id}.plugin.yaml
  source: Source/${module_id}/main.cpp
  tests: Tests/${module_id}.tests.cpp
  docs: Docs/${plugin_id}.md
capabilities:
  - engine_module_provider
  - renderer2d_provider
permissions:
  runtime:
    packaged: true
  platform:
    gpu: required
release_gate:
  require_headless_fallback: true
  forbid_native_handle_in_abi: true
sample_acceptance:
  - astra validate Plugins/${plugin_id}
  - astra run Samples/CustomizationPlugin --renderer ${provider_id}
```

插件作者验收：

- 新插件可由 wizard 创建、构建、加载、诊断、打包或被 release gate 拒绝。
- 插件错误必须显示 descriptor path、module id、capability、permission 和修复建议。

## 4. ModuleManager

生命周期：

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

`ModuleManager` 输出诊断给 Editor、CLI、MCP、Cook 和 Release Gate。

Module states：

```text
Discovered
  -> DescriptorValid
  -> DependenciesResolved
  -> BinaryLoaded
  -> Initialized
  -> ExtensionsRegistered
  -> Active
  -> Deactivating
  -> Shutdown
  -> Unloaded
```

Failure policy：

- descriptor invalid：never load binary。
- dependency missing：do not load; emit blocking diagnostic in package/release profile。
- ABI mismatch：do not call module entrypoint。
- initialize failure：call shutdown if module provided partial API。
- activate failure：module remains loaded but inactive only in development profile；release profile blocks。
- unload failure：mark as unload-blocked and prevent hot reload。

Module diagnostics must include plugin id、module id、state、entrypoint、dependency chain and suggested fix。

## 5. AstraModule C ABI

```cpp
extern "C" ASTRA_MODULE_EXPORT AstraModuleResult astra_module_main(
    const AstraModuleHostApi* host,
    AstraModuleApi* out_module);
```

ABI 规则：

- 只使用固定宽度整数、UTF-8 C 字符串、opaque handle、函数指针和 POD descriptor。
- 错误通过 result code 和 diagnostics sink 返回。
- C++ SDK 可以包装 ABI，但稳定边界仍是 C ABI。
- `ActorHandle`、`ServiceHandle`、`AssetHandle` 等均为 opaque handle。

ABI structs target shape：

```cpp
typedef struct AstraModuleHostApi {
    uint32_t abi_version;
    AstraDiagnosticsApi diagnostics;
    AstraServiceRegistryApi services;
    AstraExtensionRegistryApi extensions;
    AstraEngineModuleRegistryApi engine_modules;
    AstraAllocatorApi allocator;
} AstraModuleHostApi;

typedef struct AstraModuleApi {
    uint32_t abi_version;
    AstraStringView module_id;
    AstraModuleInitializeFn initialize;
    AstraModuleActivateFn activate;
    AstraModuleDeactivateFn deactivate;
    AstraModuleShutdownFn shutdown;
    AstraModuleQueryFn query;
} AstraModuleApi;
```

ABI lifetime rules：

- Host owns registry handles。
- Module owns only objects it allocates and explicitly registers with destroy callbacks。
- Strings returned by module must either be static, host-allocated with explicit free callback, or copied into host-owned descriptors during registration。
- Module callbacks must be valid until shutdown returns。
- No callback may outlive module unload。
- Module must unregister or deactivate runtime-facing extensions before shutdown。

ABI compatibility tests：

- missing entrypoint。
- wrong ABI version。
- missing required lifecycle function。
- descriptor says capability but module does not register it。
- module registers forbidden native handle field。
- module returns invalid UTF-8 string。

## 6. ServiceRegistry

`ServiceRegistry` 提供运行时服务发现：

- Core diagnostics、logging、configuration。
- Asset、VFS、ResourceHandle。
- EventBus、StateMachineRuntime、ActorWorld。
- ScriptRuntimeHost、Script API provider。
- Renderer2D、FilterGraph、Audio、Text。
- Save/Replay、AgentAudit、SecretProvider。

服务按 capability 暴露最小接口。模块不能直接包含内部对象所有权。

Service descriptor：

```yaml
service_id: astra.asset.registry
provider_module: astra.asset
capability: asset_registry_read
lifetime: engine
threading: main_thread_or_read_lock
permissions_required:
  - project_read
abi:
  handle_kind: AstraServiceHandle
  version: astra.asset.registry.v1
```

Resolve flow：

```text
Module request
  -> Check module state active/initializing
  -> Check requested service id and version
  -> Check capability and permission
  -> Return capability view / opaque handle
  -> Record resolve audit
```

Service lifetime：

- `engine`：valid until engine shutdown。
- `runtime_world`：valid until RuntimeWorld destroy。
- `project`：valid until project close。
- `session`：valid until Editor/MCP session ends。

Resolve errors are diagnostics, not crashes. Release Gate can replay service resolve policy from descriptors without loading unsafe binaries.

## 7. ExtensionRegistry

第一阶段扩展点：

- `ActorTypeProvider`
- `ActorComponentProvider`
- `StateMachineProvider`
- `ScriptRuntimeProvider`
- `ScriptApiProvider`
- `PresentationLibraryProvider`
- `FilterProvider`
- `RenderPassProvider`
- `AssetImporter`
- `PackageReader`
- `CookProcessor`
- `AIProvider`
- `RuntimeIntentValidator`
- `AgentAuditSink`
- `CompatRuntimeProvider`
- `LegacyApiMapper`
- `ModernizationProfileProvider`
- `EditorPanelProvider`
- `McpResourceToolPromptProvider`

Provider contract 摘要：

- `IEditorPanelProvider`：注册 panel id、菜单位置、command ids、required services、layout defaults。
- `IAssetImporter`：声明 source extensions、asset type、preset schema、sidecar defaults、diagnostics。
- `ICookProcessor`：声明 input asset type、output artifact、DDC key、package eligibility。
- `IScriptRuntimeProvider`：声明 runtime id、source types、host API needs、debug/snapshot capability。
- `IPresentationLibraryProvider`：声明 command/event kinds、state machines、preview support。
- `IRenderer2DProvider` / `ITextLayoutProvider` / `IAudioProvider`：声明 slot id、backend features、headless support、packaged eligibility。
- `IMcpToolProvider`：声明 resources/tools/prompts、session requirements、mutating behavior、audit policy。
- `IAIProvider`：声明 modality、network/offline, runtime eligibility、secret requirements、streaming support。

每个 provider 必须有 permission、release gate rule、sample plugin expectation 和 diagnostics code。

Extension descriptor：

```yaml
extension_id: example.importer.sprite
kind: AssetImporter
provider_module: example.runtime
contract: IAssetImporter
version: 1
required_services:
  - astra.asset.registry
permissions:
  project_write: true
release:
  packaged_eligible: false
hot_reload:
  level: asset
diagnostics:
  code_prefix: ASTRA_EXAMPLE_IMPORTER
```

Registration rules：

- extension id unique per project。
- kind must be registered by engine or module with capability。
- descriptor payload must validate against extension-kind schema。
- duplicate policy is explicit: reject、replace-in-development、or append-multiple。
- extension unregister is allowed only before activation or during controlled shutdown。
- runtime-safe extension cannot depend on editor-only service。

Query filters：

- kind。
- provider module。
- package eligibility。
- required feature。
- runtime/editor availability。
- release profile。

Provider descriptor 公共字段：

```yaml
provider_id: project.renderer.dx11
contract: IRenderer2DProvider
module_id: project.renderer
slot_id: astra.renderer2d
display_name: Project DX11 Renderer
required_services: [astra.diagnostics, astra.asset_registry]
permissions:
  runtime:
    packaged: true
  platform:
    gpu: required
features:
  headless_fallback: false
  frame_capture: true
  hot_reload_level: asset
diagnostics:
  unsupported_texture_format: ASTRA_RENDERER_001
release_gate:
  require_binary_hash: true
  require_abi_compatibility: true
```

## 8. EngineModuleSlot 替换机制

Engine Module Slot 是类似 UE 的可替换引擎模块选择层。它不替代
`ServiceRegistry` 或 `ExtensionRegistry`，而是在这些 registry 之上记录“某类引擎能力应由哪个
provider 承担”。服务和扩展仍然保持唯一 ID；slot selection 只返回 provider metadata，由消费者按
slot 查询后决定使用哪个 service、extension 或 presentation library。

基本概念：

- `EngineModuleSlot`：可替换能力槽位，例如 `astra.renderer2d`、`astra.text_layout`、
  `astra.audio`、`astra.compat.artemis.runtime`。
- `EngineModuleProvider`：某个模块对 slot 的候选实现，声明 `slot_id`、`extension_id`、
  `service_id` 和注册模块 ID。
- `EngineModulePolicy`：项目配置显式选择 slot 的 provider。
- `default_provider_id`：没有项目选择时使用的默认 provider。

项目配置示例：

```yaml
engine_modules:
  selections:
    astra.renderer2d: project.renderer.dx11
    astra.text_layout: project.text.japanese_ruby
    astra.compat.artemis.runtime: astra.compat.artemis.default
```

动态模块可通过 C ABI 注册 slot 和 provider。注册 slot 需要
`engine_module_slot_provider` capability；注册 provider 需要 `engine_module_provider`
capability。provider 必须引用已经注册的 slot。跨插件顺序通过 dependencies 和 load phase 保证。

选择规则：

- 同一 slot ID 或 provider ID 重复注册会失败。
- policy 中的 slot 和 provider 必须都存在。
- policy 选择的 provider 必须属于对应 slot。
- 未被 policy 选择的 slot 使用 `default_provider_id`。
- v1 不做 priority 自动抢占，不按加载顺序隐式替换。
- selection 不覆盖或移除 `ServiceRegistry` 中已有 service。

适合替换的 slot：

- Renderer2D、TextLayout、Audio。
- AssetResolver、PackageReader、FilterProvider。
- ScriptRuntimeProvider。
- Expansion track：CompatRuntimeProvider、LegacyApiMapper。
- VN DialogueSystem、ChoiceSystem、PresentationLibrary。
- EditorPanelProvider。
- Expansion track：CompatibilityInspector。

不可替换的核心：

- ModuleManager、C ABI、ServiceRegistry、ExtensionRegistry。
- PropertySystem 基础类型协议。
- Core diagnostics、logging、config、path、time。
- Platform lifecycle、thread scheduler、render device core。

Release Gate 应检查 slot/provider 依赖闭包、权限、packaged eligibility、policy 引用有效性、
默认 provider 是否存在，以及发布配置中是否存在未授权的 runtime replacement。Compat slot/provider
检查只在启用 legacy expansion build 时进入发布门禁。

## 9. PropertySystem

PropertySystem 替代完整 UObject：

- 稳定 `TypeId`、`PropertyId`、enum metadata。
- 描述 scalar、localized text、asset ref、struct、array、map、tagged union。
- 生成 JSON Schema。
- 驱动 Inspector、MCP 字段编辑、diff、audit、serialization。
- 标记 `ai_editable`、`tool_generated`、`read_only`、`requires_review`。

## 10. 热重载

分层：

```text
Level 1: Asset Hot Reload
Level 2: Script / Graph / Timeline Hot Reload
Level 3: Filter Profile / Shader Hot Reload
Level 4: Presentation Library Hot Reload
Level 5: Runtime Module Development Reload
```

不热重载：

- Memory manager、ModuleManager、Render device core、Platform layer、thread scheduler。

## 11. 打包与 Release Gate

发布包只包含启用且 runtime-safe 的模块。Editor、Developer、debug MCP、authoring-only 模块默认排除。Release Gate 校验 descriptor schema、ABI version、权限、依赖闭包、packaged eligibility、binary hash 和模块策略。

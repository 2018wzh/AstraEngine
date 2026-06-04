# 扩展与动态模块系统设计

## 1. 目标

AstraEngine 使用动态模块作为默认扩展模型。模块系统必须支持通用 2D 引擎能力、VN Presentation、AI Provider、运行时 Intent、旧 VN 模拟器、现代化滤镜、Editor 扩展和 Cook/Package 工具，同时保持 Core 干净、ABI 稳定和发布可审计。

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

Plugins/BGICompat
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

## 6. ServiceRegistry

`ServiceRegistry` 提供运行时服务发现：

- Core diagnostics、logging、configuration。
- Asset、VFS、ResourceHandle。
- EventBus、StateMachineRuntime、ActorWorld。
- ScriptRuntimeHost、Script API provider。
- Renderer2D、FilterGraph、Audio、Text。
- Save/Replay、AgentAudit、SecretProvider。

服务按 capability 暴露最小接口。模块不能直接包含内部对象所有权。

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

## 8. PropertySystem

PropertySystem 替代完整 UObject：

- 稳定 `TypeId`、`PropertyId`、enum metadata。
- 描述 scalar、localized text、asset ref、struct、array、map、tagged union。
- 生成 JSON Schema。
- 驱动 Inspector、MCP 字段编辑、diff、audit、serialization。
- 标记 `ai_editable`、`tool_generated`、`read_only`、`requires_review`。

## 9. 热重载

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

## 10. 打包与 Release Gate

发布包只包含启用且 runtime-safe 的模块。Editor、Developer、debug MCP、authoring-only 模块默认排除。Release Gate 校验 descriptor schema、ABI version、权限、依赖闭包、packaged eligibility、binary hash 和模块策略。

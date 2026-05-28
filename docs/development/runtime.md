# Runtime 开发说明

状态：Draft

## 模块边界

Runtime 不依赖 Editor。当前第一版 Runtime targets 包括 Core、ApplicationCore、PlatformSDL3、RHI、Renderer2D、TextCore、AudioCore、AssetCore、VFS、AssetRegistry、ModuleRuntime、ExtensionRegistry、VNPropertySystem、VNRuntimeServices、Bootstrap 和 AstraRuntime。

引擎 Runtime 模块以 DLL 形式构建，插件以独立动态库形式加载。`AstraGame` 初始化时只创建 `RuntimeProviderRegistry`、`ExtensionRegistry` 和 `ModuleManager`，然后扫描 `build/Plugins` 下的插件 descriptor。默认 Platform、Renderer2D、Audio 和 ProjectContent 后端由 `DefaultRuntimeProvidersPlugin` 注册，不在 `AstraGame` 或 `AstraRuntimeSession` 中硬编码构造。

Provider 初始化路径：

```text
PluginDescriptor
-> ModuleManager::discover
-> LoadLibrary
-> astra_module_main / initialize / activate
-> astra_register_native_runtime_providers
-> RuntimeProviderRegistry
```

`astra_register_native_runtime_providers` 是 engine-native 插件入口，只用于随引擎同工具链构建的 Runtime provider 插件。第三方模块的稳定边界仍是 `AstraModule` C ABI。

## Session 生命周期

`AstraRuntimeSession` 第一版生命周期：

```text
load_project -> start -> tick / advance / choose
-> save_snapshot -> restore_snapshot -> shutdown
```

数据流：

```text
Project manifest + Config + sidecars
-> ProjectContentProvider
-> AssetRegistry / VFS
-> .astra parser
-> RuntimeCommand
-> VNRuntimeServices
-> ECS-backed state
-> RenderSnapshot / AudioRequest / SaveSnapshot
```

## RuntimeCommand

第一版命令覆盖：

- `ShowBackground`
- `ShowCharacter`
- `PlayBGM`
- `PlaySFX`
- `ShowDialogue`
- `PresentChoice`
- `SetVariable`
- `JumpScene`

Runtime Services 内部使用 EnTT，但 public facade、RuntimeCommand、render/audio/save DTO 不暴露 EnTT 类型。

## Schedule

固定 schedule：

```text
Input
Script
CommandApply
Animation
Audio
RenderExtract
SaveSnapshot
Cleanup
```

Headless test 不初始化 Renderer2D 或 AudioCore，也能执行 RuntimeCommand、schedule、选择分支和 save/restore。

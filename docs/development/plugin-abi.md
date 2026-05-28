# 插件 ABI

状态：Draft

## ABI 边界

第一版 ABI 版本为 `ASTRA_MODULE_ABI_VERSION = 1`，入口固定为：

```cpp
extern "C" ASTRA_MODULE_EXPORT AstraResultCode astra_module_main(
    const AstraModuleHostApi* host,
    AstraModuleApi* out_module);
```

ABI 只允许：

- 固定宽度整数。
- UTF-8 `AstraStringView`。
- `AstraByteSpan`。
- opaque handle。
- 函数指针。
- POD descriptor。
- result code 和 diagnostic sink。

ABI 不允许暴露 STL、exceptions、RTTI、EnTT、SDL、Renderer2D、AudioCore、Editor 对象或跨边界 C++ ownership。

## 生命周期

ModuleManager 加载顺序：

```text
Discover -> ValidateDescriptor -> ResolveDependencies -> CheckVersion
-> LoadBinary -> astra_module_main -> initialize -> activate
-> deactivate -> shutdown -> unload
```

当前示例模块位于 `Engine/Plugins/Examples/ExampleRuntime`，构建后 descriptor 会复制到 `build/Plugins/ExampleRuntime/ExampleRuntime.plugin.yaml`。

默认运行时后端位于 `Engine/Plugins/Runtime/DefaultRuntimeProviders`，构建后 descriptor 会复制到 `build/Plugins/DefaultRuntimeProviders/DefaultRuntimeProviders.plugin.yaml`。它仍通过 `astra_module_main` 声明扩展：

- `ASTRA_EXTENSION_PLATFORM_PROVIDER`
- `ASTRA_EXTENSION_RENDERER_PROVIDER`
- `ASTRA_EXTENSION_AUDIO_PROVIDER`
- `ASTRA_EXTENSION_PROJECT_CONTENT_PROVIDER`

实际 Provider 对象注册使用 engine-native 入口 `astra_register_native_runtime_providers`。该入口不是公共插件 ABI，不允许第三方模块依赖；它只服务随引擎构建、同编译器和同 Runtime DLL 集合部署的后端插件。

## Descriptor

插件 descriptor 使用 YAML，并由 `Schemas/plugin.schema.json` 描述字段形状。第一版字段包括：

- `id`
- `display_name`
- `version`
- `astra_api`
- `modules`
- `type`
- `entrypoint`
- `load_phase`
- `capabilities`
- `permissions`
- `platforms`
- `dependencies`

能力注册必须与 descriptor 中的 capability 匹配。当前测试覆盖真实动态库加载、生命周期调用、默认 Runtime Provider 插件注册，以及 `RuntimeCommandSource` / `VNPropertyType` 扩展注册。

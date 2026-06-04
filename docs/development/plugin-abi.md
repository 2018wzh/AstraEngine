# 插件 ABI

## 1. 当前 ABI 目标

Phase 1 插件 ABI 只解决三件事：

- 动态模块发现与加载
- 宿主向模块暴露受控服务访问
- 模块向宿主注册受控扩展点

当前 ABI 定义在 [AstraModuleABI.h](/E:/Documents/AstraEngine/Engine/Runtime/ModuleRuntime/Public/Astra/ModuleRuntime/AstraModuleABI.h)。

## 2. 当前入口

每个模块导出：

```c
AstraResultCode astra_module_main(const AstraModuleHostApi* host,
                                  AstraModuleApi* out_module);
```

宿主提供：

- `abi_version`
- diagnostics callback
- `register_extension`
- `get_service`

模块返回：

- `module_id`
- `module_context`
- `initialize`
- `activate`
- `deactivate`
- `shutdown`

## 3. 当前扩展点种类

Phase 1 只保留三类扩展：

- `service_extension`
- `property_type_provider`
- `editor_metadata_provider`

对应 C ABI 枚举：

- `ASTRA_EXTENSION_SERVICE_EXTENSION`
- `ASTRA_EXTENSION_PROPERTY_TYPE_PROVIDER`
- `ASTRA_EXTENSION_EDITOR_METADATA_PROVIDER`

## 4. 宿主服务访问

模块不能直接访问宿主内部对象，只能通过 `get_service` 按服务 ID 获取 opaque handle。

当前宿主服务 ID 包括：

- `astra.platform.window`
- `astra.platform.input`
- `astra.platform.filesystem`
- `astra.platform.timer`
- `astra.platform.thread`
- `astra.platform.dynamic_library`
- `astra.property.registry`

## 5. capability 与 permission

模块 descriptor 决定：

- 它声明了哪些 capability
- 它请求了哪些 permission

`ServiceRegistry` 和 `ExtensionRegistry` 都会校验这些边界。

Phase 1 示例模块只声明：

- `service_extension`
- `property_type_provider`

## 6. 仍然禁止的跨 ABI 对象

当前仍然禁止跨 ABI 直接传递：

- STL ownership
- C++ Actor 指针
- renderer / audio native handle
- Editor widget
- future Scene / Runtime internals

这条约束即使在 Phase 2+ 也不应放松。

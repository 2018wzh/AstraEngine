# Runtime 现状

## 1. 当前实现范围

当前代码中的“runtime”指 Phase 1 基础层，不是完整 Scene/Runtime 目标架构。

已实现：

- `Astra_Core`
- `Astra_Platform`
- `Astra_ModuleRuntime`
- `Astra_PropertySystem`

未实现：

- `ActorWorld`
- `EventBus` 作为场景运行时主线
- `StateMachineRuntime`
- `Blackboard`
- `ControlPolicy`
- `Save / Load / Replay`
- `ScriptRuntimeHost`

## 2. 当前宿主链路

当前可运行链路如下：

```text
Host program
  -> create_default_platform_services()
  -> ServiceRegistry
  -> ExtensionRegistry
  -> PropertyRegistry
  -> ModuleManager.discover()
  -> ModuleManager.load_discovered()
  -> module initialize / activate
```

对应样例可见 [AstraPhase1Smoke main.cpp](/E:/Documents/AstraEngine/Engine/Programs/AstraPhase1Smoke/Private/main.cpp)。

## 3. 当前模块职责

### Core

提供：

- `Error` / `Expected`
- `Diagnostic` / `DiagnosticSink`
- `Assert`
- `Log`
- `Path`
- `Config`
- `Time`

### Platform

提供接口族：

- `IWindowService`
- `IInputService`
- `IFileSystemService`
- `ITimerService`
- `IThreadService`
- `IDynamicLibraryService`

默认实现通过 SDL3 和本地动态库加载器提供，公开头文件不暴露 SDL 类型。

### ModuleRuntime

提供：

- plugin descriptor 解析
- `ModuleManager`
- `ServiceRegistry`
- `ExtensionRegistry`
- `AstraModule` C ABI

### PropertySystem

提供：

- `TypeId`
- `PropertyId`
- `PropertyTypeKind`
- `PropertyFlags`
- `PropertyDescriptor`
- `TypeDescriptor`
- `PropertyRegistry`

## 4. 当前不是运行时中心的东西

当前代码里不存在这些旧中心抽象：

- `VNRuntimeServices`
- `RuntimeCommand` 作为唯一主线
- `Bootstrap`
- `RuntimeProviderRegistry`

这次 Phase 1 已经把它们从默认主线中移除。

## 5. 下一阶段接口位置

Phase 2 计划把真正的运行时基座加入：

- `Scene`
- `ActorWorld`
- `EventBus`
- `StateMachineRuntime`
- `Blackboard`
- `ControlPolicy`
- `Director`

届时本文件应升级为完整 Runtime 开发文档，而不是 Phase 1 现状说明。

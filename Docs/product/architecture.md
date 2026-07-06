# 总体架构

AstraEngine 系列采用“共享引擎核心 + 垂直产品 + 平台壳 + 扩展套件”的结构。AstraEngine 仓库维护公共契约，子仓只实现自己的产品面。

## 多仓职责

```text
AstraEngine
  core/runtime/asset/media/script/plugin/test contracts
AstraVN
  .astra canonical source, VN preset, Luau policy, commercial VN baseline
AstraEditor
  Qt/QML creator editor, PIE, inspector, graph/timeline, release UI
AstraPlatform
  desktop/mobile/web/experimental native shells and platform decode
AstraEMU
  manager, RuntimeWorld-driven family plugins, Artemis v1 family, Luau patch/decode API
```

## 运行链路

```text
Text-first source (.astra/.yaml/assets)
  -> Import/Cook
  -> Binary package
  -> RuntimeWorld
  -> Actor/Component + StateMachine
  -> PresentationCommand / AudioCommand / RuntimeEvent
  -> Renderer2D / TextLayout / AudioGraph / FilterGraph providers
  -> Save / Replay / ReleaseReport
```

RuntimeWorld 是组合 facade，不是全局单例。Editor、CLI、MCP、平台壳和测试框架都通过同一 public API 创建和驱动它。

## Core 边界

Core 包含基础类型、diagnostics、stable id、schema、migration、PropertySystem、ServiceRegistry、ExtensionRegistry、EngineModuleSlot 和插件加载策略。Core 不知道 VN、Editor、MCP、AI、Luau、legacy VM 或任何具体平台后端。

## Runtime 边界

Runtime 拥有 World、Scene、Actor、Component、StateMachine、EventBus、Scheduler、Director、ControlPolicy、Save/Replay 和 Debug API。Tokio task 可以服务 IO、decode、network 和工具任务，但 Runtime deterministic state 只在固定 tick 边界消费有序结果。

## Module Slot

可替换能力通过 EngineModuleSlot 和 ExtensionRegistry 明确选择，不按加载顺序抢占。默认 slot 包括 Renderer2D、TextLayout、AudioOutput、DecodeProvider、ScriptRuntime、PresentationLibrary、AssetResolver、AIProvider、MCPToolProvider、AstraEMU LegacyFamilyPlugin 和可选 EMUCoreBridge。

## 产品边界

AstraVN 是原生 VN 垂直模块。AstraEMU 是旧 VN 兼容与现代化套件。AstraEMU Manager 创建并驱动 RuntimeWorld，family plugin 只注册 VFS、legacy script、StateMachine action、media mapper 和 snapshot codec；NativeVN 创作流程不能依赖 EMU family plugin。

## v1 验收边界

全系列 v1 同时要求 EngineCore deterministic gate、NativeVN commercial baseline、UE 级 Editor workflow、六平台 profile gate、AI/MCP audit gate 和 Artemis engine-native family gate。任一产品线可以独立开发，但 release 口径由本仓 contracts、implementation specs 和 status matrix 统一定义。

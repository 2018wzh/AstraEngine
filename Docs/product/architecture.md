# 总体架构

AstraEngine 系列采用“共享引擎核心 + 垂直产品 + 平台壳 + 扩展套件”的结构。AstraEngine 仓库维护公共契约，子仓只实现自己的产品面。

## 多仓职责

```text
AstraEngine
  core/runtime/asset/media/script/plugin/test contracts
AstraVN
  .astra canonical source, VN preset, Lua extension, commercial VN baseline
AstraEditor
  Qt/QML creator editor, PIE, inspector, graph/timeline, release UI
AstraPlatform
  desktop/mobile/web/experimental native shells and platform decode
AstraEMU
  manager, out-of-process compat cores, family adapters, Lua patch/decode API
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

Core 包含基础类型、diagnostics、stable id、schema、migration、PropertySystem、ServiceRegistry、ExtensionRegistry、EngineModuleSlot 和插件加载策略。Core 不知道 VN、Editor、MCP、AI、Lua、legacy VM 或任何具体平台后端。

## Runtime 边界

Runtime 拥有 World、Scene、Actor、Component、StateMachine、EventBus、Scheduler、Director、ControlPolicy、Save/Replay 和 Debug API。Tokio task 可以服务 IO、decode、network 和工具任务，但 Runtime deterministic state 只在固定 tick 边界消费有序结果。

## Module Slot

可替换能力通过 EngineModuleSlot 明确选择，不按加载顺序抢占。默认 slot 包括 Renderer2D、TextLayout、AudioOutput、DecodeProvider、ScriptRuntime、PresentationLibrary、AssetResolver、AIProvider、MCPToolProvider 和 AstraEMU CompatCoreBridge。

## 产品边界

AstraVN 是原生 VN 垂直模块。AstraEMU 是旧 VN 兼容与现代化套件。两者共享 Runtime、Media、Script、Save/Replay、Release Gate 语义，但 NativeVN 创作流程不能依赖 EMU family core。

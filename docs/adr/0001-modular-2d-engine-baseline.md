# ADR 0001: Modular 2D Engine Baseline

Status: Accepted

## Context

AstraEngine 的目标是高度可定制化的模块化 2D 引擎，视觉小说是第一垂直模块。引擎需要支持传统 VN、互动叙事、AI 协作、运行时受控 AI 内容、旧 VN 模拟器和现代化表现。

## Decision

AstraEngine 采用以下基线：

- Core 提供 foundation、ModuleManager、ServiceRegistry、PropertySystem 和 serialization。
- Scene 提供 Actor/Component、World、Scene、Prefab 和 Actor lifecycle。
- Runtime 提供 EventBus、StateMachineRuntime、Blackboard、ControlPolicy、Director、Save/Replay。
- Script 提供 ScriptRuntimeHost，支持 Astra Native、Lua、legacy VM 和自定义运行时。
- Media 提供 Renderer2D、Text、Audio、RenderGraph 和 FilterGraph。
- AstraVN 是第一垂直模块，不进入 Core。

源码级 CMake 模块只用于引擎内部或同工具链实验模块。项目级扩展默认走动态模块。

## Consequences

- 旧的 VN-first runtime 命名不能作为长期架构中心。
- Core 不依赖 VN、AI、Live2D、Lua 或旧 VM。
- 文档和代码演进应优先围绕通用 2D Core，再构建 AstraVN。

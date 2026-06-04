# ADR 0010: Core Owns Actor, Component, and StateMachine Concepts

Status: Accepted

## Context

Actor/Component 不是仅服务编辑器的 facade，而是目标架构的公开运行时对象模型。状态机是叙事、演出、UI、交互和 AI Intent 的核心运行抽象。

## Decision

Core/Scene/Runtime 提供 Actor、Component、StateMachine 的基础概念：

- Core 提供 ID、PropertySystem、serialization 和 diagnostics。
- Scene 提供 ActorWorld、ActorId、ActorTypeId、ComponentDescriptor、ActorHandle。
- Runtime 提供 EventBus、StateMachineRuntime、Blackboard、ControlPolicy、Director 和 Save/Replay。

AstraVN 在这些基础上提供 VN 专用 Actor 和状态机。动态模块通过 descriptor、state machine provider、script API provider 和 service extension 扩展，不跨 ABI 继承 C++ Actor。

## Consequences

- Actor 不下沉为完整 UObject/GC 对象体系。
- Core 仍不依赖 VN、AI、Live2D 或旧 VM。
- Save/Load、Editor、MCP 和 Compatibility 都围绕 ActorId、Component data 和 StateMachine state 工作。

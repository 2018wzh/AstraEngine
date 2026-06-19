# ADR 0004: Actor/Component + StateMachineRuntime, ECS for Hotspots

Status: Accepted

## Context

早期设计曾把内部 ECS 作为 Runtime Services 的中心。目标架构改为模块化 2D 引擎后，公开运行时模型需要更贴近创作、编辑、脚本和调试：Actor 拥有 Component 和多个状态机，事件驱动状态变化。

## Decision

AstraEngine 的运行时中心是：

- Actor/Component。
- EventBus。
- StateMachineRuntime。
- Blackboard。
- ControlPolicy。
- Director。

ECS / Data-Oriented system pack 只用于性能热点和批量处理，例如粒子、批量动画、FilterGraph pass 调度、legacy timeline 推进。ECS 不作为公开 authoring model，也不作为唯一权威运行时中心。

## Consequences

- Save/Load 使用 ActorId、Component data、StateMachine state 和 Blackboard，不保存 ECS entity 原始值。
- 脚本、Editor、MCP 和插件 API 不暴露内部 ECS。
- 局部 ECS 与 Actor 世界通过稳定 ID、event、snapshot 和 service DTO 同步。



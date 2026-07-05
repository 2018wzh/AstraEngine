# ADR 0004: Actor/Component + StateMachine 是 Runtime 权威模型

## Context

VN、互动叙事、Editor Inspector、Save/Replay 和自动化测试都需要可解释的运行时对象模型。纯 ECS 不适合作为创作者和调试器的主要模型。

## Decision

Runtime 使用 Actor/Component + StateMachine。局部 ECS 只用于热点批处理，不进入 public save、Inspector 或脚本对象模型。

## Consequences

状态机 trace、Director、ControlPolicy、Save/Replay 和 Debugger 可以围绕同一模型实现。性能热点仍可用 ECS 优化。

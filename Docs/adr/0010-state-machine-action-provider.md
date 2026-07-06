# ADR 0010: StateMachine Action Provider

## Status

Accepted.

## Context

Stage 1 已经有 flat StateMachine、EventQueue、AwaitToken 和 save/replay。要支撑 2D gameplay 插件，状态机 action 不能停留在 host 内置函数表：插件需要声明 action descriptor，Runtime 需要用固定 tick、可序列化 payload 和受限 mutation context 执行它们。

本决策只补强 Stage 1 EngineCore。层级状态、并行状态、pushdown stack、rollback netcode、physics/collision、renderer/audio provider 继续留在后续产品线，不进入本轮边界。

## Decision

- StateMachine 继续保持 flat FSM。一个 transition 可以包含 `Vec<ActionInvocation>`，Runtime 按定义顺序执行。
- Guard 保持同步纯函数，只读取 event、Actor snapshot 和 Blackboard。
- Action 执行只通过 host 提供的 `DeterministicActionContext`。context 允许 Actor/Component mutation、Blackboard 写入、Event 发射、AwaitToken 创建、PresentationCommand 提交、delayed event schedule/cancel。
- Runtime action registry 支持 provider 安装和卸载。`RuntimeWorld::register_action` 注册 host adapter，`RuntimeWorld::unregister_action_provider` 清理该 provider 的 action。
- FFI 插件不接收 `RuntimeWorld`、Actor 指针、native handle 或 trait object。插件只拿 postcard 编码的 request bytes，返回 postcard 编码的 result bytes。
- FFI action result 是 action trace 加 effect list。host adapter 按顺序把 effect 应用到 `DeterministicActionContext`。
- Delayed event queue 进入 Runtime snapshot 和 save/replay。队列按 `(due_tick, sequence, id)` drain，并在固定 tick 边界进入 EventQueue。
- Action 失败时，当前 machine 不迁移，Runtime 写 blocking diagnostic，tick 继续处理其他 machine。失败 transition 的候选 mutation 不提交。

## Consequences

- Stage 1 能覆盖通用 2D 插件最小但完整的 deterministic action runtime：插件能驱动 gameplay state、事件、presentation 和等待点，但不能绕过 EngineCore。
- Save/replay 能保存 delayed event queue，因此 timer 类 gameplay 不依赖平台 task order。
- Action provider 的 ABI 边界以 bytes 和 stable value 为准，Rust trait 只存在于 host adapter 侧。
- 后续如果引入层级状态或并行状态，需要新增 ADR，并保持现有 flat FSM save 兼容路径。

## Verification

```bash
cargo test -p astra-runtime state_machine_tick
cargo test -p astra-runtime delayed_event
cargo test -p astra-plugin ffi_action_provider
cargo test -p astra-test native_smoke
```

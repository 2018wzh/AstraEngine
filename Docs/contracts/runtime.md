# Runtime Contract

Runtime 的权威模型是 Actor/Component + StateMachine。局部 ECS 可以优化批量 transform、粒子、sprite sorting 或音频 voice 更新，但不能进入 public save、Inspector 或脚本对象模型。

## Public API 草案

```rust
pub struct RuntimeWorld;
pub struct RuntimeConfig;
pub struct ActorId(pub StableId);
pub struct ComponentId(pub StableId);
pub struct TickInput { pub fixed_step: u64, pub delta_ns: u64, pub seed: u64 }
pub struct TickReport {
    pub step: u64,
    pub state_hash: Hash128,
    pub event_hash: Hash128,
    pub presentation_hash: Hash128,
    pub diagnostics: Vec<Diagnostic>,
}

impl RuntimeWorld {
    pub fn create(config: RuntimeConfig, package: PackageHandle) -> Result<Self, RuntimeError>;
    pub fn tick(&mut self, input: TickInput) -> Result<TickReport, RuntimeError>;
    pub fn apply_input(&mut self, input: PlayerInput) -> Result<(), RuntimeError>;
    pub fn register_action<A: RuntimeAction + 'static>(&mut self, provider_id: impl Into<String>, action: A);
    pub fn unregister_action_provider(&mut self, provider_id: &str);
    pub fn schedule_event(&mut self, due_tick: u64, source: EventSource, payload: EventPayload) -> DelayedEventId;
    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) -> bool;
    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError>;
    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError>;
    pub fn debug_session(&self) -> RuntimeDebugSession<'_>;
}
```

字段级实现蓝图见 [Runtime API Blueprint](../implementation/runtime-api.md)、[Runtime Execution](../implementation/runtime-execution.md) 和 [StateMachine Action Provider](../implementation/state-machine-action-provider.md)。

## Actor / Component

Actor 只保存 stable id、parent/child relation、tag 和 component refs。Component payload 必须有 schema id、schema version、serde data、migration policy 和 Inspector metadata。Runtime public API 不暴露内部 arena index。

```rust
pub struct ActorSnapshot {
    pub actor_id: ActorId,
    pub name: String,
    pub tags: Vec<String>,
    pub components: Vec<ComponentId>,
}
```

Save、Inspector、Debug、MCP patch 都通过 snapshot 和 MutationLog 访问 Actor/Component。

## AwaitToken

Runtime action 可以发起异步工作，但 await 点必须显式序列化：

```rust
pub struct AwaitToken {
    pub token_id: StableId,
    pub kind: AwaitKind,
    pub requested_at_step: u64,
    pub deterministic_timeout_step: Option<u64>,
    pub replay_policy: AwaitReplayPolicy,
}
```

Tokio task 完成后只提交 `AwaitResult`。Runtime 在固定 tick 边界按 `token_id` 和 sequence 消费结果。Guard 必须是同步纯函数；Action 可以拆成 `start -> await token -> resume`。

## 状态机规则

- Guard 只读取 event payload、Actor snapshot、Blackboard、Director state。
- Transition 使用 `actions: Vec<ActionInvocation>`，同一个 transition 内按顺序执行。
- Action 只通过 `DeterministicActionContext` 改 Actor/Component、Blackboard、EventQueue、AwaitToken、PresentationCommand 和 delayed event queue。
- Runtime action provider 可以被注册和卸载；插件 action 由 host-side adapter 执行，插件不拿 `RuntimeWorld`、Actor 指针或 native handle。
- 状态机定义分双轨：引擎系统用 Rust code-first；项目 gameplay/VN 可以用 YAML/Graph 定义并 Cook 成 IR。
- Save 保存 StateMachine、Blackboard、AwaitQueue、Event trace 和 DelayedEventQueue，不保存 ECS entity、native handle 或 Future 内部状态。

## Delayed Event

`DelayedEventQueue` 用 `DelayedEventId` 标识任务，按 fixed tick 触发。Runtime 在每个 tick 开始阶段把到期事件按 `(due_tick, sequence, id)` 排序后进入 EventQueue，再交给 StateMachine。队列属于 `RuntimeSnapshot`，save/load/replay 后必须保持同一触发 tick。

## 失败策略

Unknown event、invalid payload、missing required module、missing action、action failure、schema migration failure 都是 blocking diagnostic。Action failure 不迁移当前 machine，不提交候选 mutation，tick 继续处理其他 machine。PIE 可以暂停，packaged runtime 只能按 release profile 的 fatal policy 退出或进入安全错误页。

## Release Gate

`runtime.replay.determinism`、`runtime.await.ordering`、`runtime.save_load`、`runtime.debug_snapshot`、`runtime.delayed_event`、`plugin.ffi_action_provider` 是必需检查。每个检查必须输出 step、hash、source_ref 或 diagnostic code。

# Runtime API Blueprint

Runtime API 以 `RuntimeWorld` 为组合 facade。实现者不能把它做成全局单例，也不能把 Editor、平台窗口、renderer backend 或 Luau host 放进 core crate。

## Core Types

```rust
pub struct RuntimeWorld {
    world_id: WorldId,
    actors: ActorStore,
    machines: StateMachineStore,
    events: EventQueue,
    awaits: AwaitQueue,
    delayed_events: DelayedEventQueue,
    mutations: MutationLog,
}

pub struct ActorId(pub StableId);
pub struct ComponentId(pub StableId);

pub struct ComponentRecord {
    pub component_id: ComponentId,
    pub actor_id: ActorId,
    pub schema: SchemaId,
    pub version: SchemaVersion,
    pub data: ComponentData,
}
```

Actor/Component 是 public save、Inspector 和 script 可见模型。局部 ECS 只能在 renderer、particle、sprite sorting、audio voice 等热点内部使用，输出仍要回到 Actor/Component 或 presentation command。

## Lifecycle

```rust
impl RuntimeWorld {
    pub fn create(config: RuntimeConfig, package: PackageHandle) -> Result<Self, RuntimeError>;
    pub fn mount_module(&mut self, slot: EngineModuleSlot, binding: ValidatedModuleBinding) -> Result<(), RuntimeError>;
    pub fn register_action<A: RuntimeAction + 'static>(&mut self, provider_id: impl Into<String>, action: A);
    pub fn unregister_action_provider(&mut self, provider_id: &str);
    pub fn schedule_event(&mut self, due_tick: u64, source: EventSource, payload: EventPayload) -> DelayedEventId;
    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) -> bool;
    pub fn tick(&mut self, request: TickRequest) -> Result<TickReport, RuntimeError>;
    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError>;
    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError>;
    pub fn replay(&mut self, replay: ReplayInput) -> Result<ReplayReport, RuntimeError>;
    pub fn debug_session(&mut self) -> RuntimeDebugSession<'_>;
}
```

`mount_module` 只接收 typed slot 和经过 registry selection、packaged eligibility、capability、package/target/profile identity、engine/rustc/feature/ABI fingerprint 校验的 binding token，不接收 provider 字符串或 native handle。缺失必需 slot、重复挂载、slot/token 不一致和 context mismatch 都是 blocking diagnostic。`tick` 只接收 typed `TickRequest`；input、await 和 provider output 没有 tick 外公开注入 API。Runtime 在任何 mutation 之前校验 lifecycle mode、strict ingress order、连续 fixed step、session seed、delta 范围和 required slot；执行期错误会恢复 tick 前 snapshot。load 后第一 tick 使用一次 `RestoreContinuation`；provider-free replay 使用 `Replay` 与 recorded output，且整个 transcript 失败时恢复 replay 调用前 world。

## State Machine

```rust
pub struct StateMachineDefinition {
    pub id: StableId,
    pub owner: ActorId,
    pub states: Vec<StateDefinition>,
    pub transitions: Vec<TransitionDefinition>,
    pub initial_state: StableId,
}

pub struct TransitionDefinition {
    pub from: StableId,
    pub to: StableId,
    pub guard: GuardExpr,
    pub actions: Vec<ActionInvocation>,
    pub source_ref: Option<SourceRef>,
}
```

Guard 是同步纯函数。Action 只通过 `DeterministicActionContext` 创建 event、Actor/Component mutation、Blackboard write、PresentationCommand、AwaitToken 或 delayed event。Action 不能直接阻塞等待 provider。

```rust
pub trait RuntimeAction: Send + Sync {
    fn descriptor(&self) -> ActionDescriptor;
    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError>;
}
```

同一 transition 的 action 按定义顺序执行。Runtime 使用候选 world state 执行 action，全部成功后才提交 mutation 和 state transition。失败时写 blocking diagnostic，当前 machine 不迁移，其他 machine 继续 tick。

## Delayed Event Queue

```rust
pub struct ScheduledEvent {
    pub id: DelayedEventId,
    pub due_tick: u64,
    pub sequence: u64,
    pub source: EventSource,
    pub payload: EventPayload,
}
```

`DelayedEventQueue` 每 tick 按 `(due_tick, sequence, id)` drain 到 EventQueue。队列进入 `RuntimeSnapshot` 和 save/replay，timer 类 gameplay 不依赖 task completion order。

## Debug Session

```rust
pub trait RuntimeDebugSession {
    fn actors(&self) -> Vec<ActorSnapshot>;
    fn components(&self, actor: ActorId) -> Vec<ComponentSnapshot>;
    fn state_machines(&self, actor: ActorId) -> Vec<StateMachineSnapshot>;
    fn event_trace(&self, range: StepRange) -> Vec<RuntimeEvent>;
    fn source_ref(&self, id: StableId) -> Option<SourceRef>;
}
```

Debug API 返回 snapshot，不返回内部指针。Editor 可以暂停 PIE 并读取 snapshot，不能通过 Debug API 绕过 MutationLog。

## Checks

```bash
cargo test -p astra-runtime world_actor
cargo test -p astra-runtime state_machine_tick
cargo test -p astra-runtime delayed_event
cargo test -p astra-runtime await_token
cargo test -p astra-runtime save_replay
```

Expected report: 同 seed、同 package、同 input 生成相同 state/event/presentation hash；hash mismatch 能定位 step、event id、actor/component 和 source_ref。

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
    pub fn mount_module(&mut self, slot: EngineModuleSlot, provider: ProviderRef) -> Result<(), RuntimeError>;
    pub fn tick(&mut self, input: TickInput) -> Result<TickReport, RuntimeError>;
    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError>;
    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError>;
    pub fn replay(&mut self, replay: ReplayInput) -> Result<ReplayReport, RuntimeError>;
    pub fn debug_session(&mut self) -> RuntimeDebugSession<'_>;
}
```

`mount_module` 只接收 slot 和 provider reference，不接收 native handle。缺失必需 slot 是 blocking diagnostic。

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
    pub action: ActionId,
    pub source_ref: SourceRef,
}
```

Guard 是同步纯函数。Action 可以创建 event、mutation、presentation/audio command、AwaitToken 或 Fence。Action 不能直接阻塞等待 provider。

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
cargo test -p astra-runtime await_token
cargo test -p astra-runtime save_replay
```

Expected report: 同 seed、同 package、同 input 生成相同 state/event/presentation hash；hash mismatch 能定位 step、event id、actor/component 和 source_ref。

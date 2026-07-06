# StateMachine Action Provider

本页说明 Stage 1 已实现的状态机 action provider 边界。目标是让通用 2D gameplay 插件能参与 deterministic Runtime，而不把 world 指针、native handle 或插件 trait object 穿过 ABI。

## Runtime Model

`TransitionDefinition` 使用 `actions: Vec<ActionInvocation>`。同一个 transition 命中后，Runtime 按数组顺序执行 action。所有 action 成功后才迁移到 `to` state；任一 action 失败时，当前 machine 保持原 state，候选 Actor/Component、Blackboard、event、presentation、await 和 delayed event mutation 都不提交。

`DeterministicActionContext` 是唯一 mutation 入口：

```rust
pub struct DeterministicActionContext<'a>;

impl DeterministicActionContext<'_> {
    pub fn create_actor(&mut self, name: impl Into<String>, tags: Vec<String>) -> ActorId;
    pub fn attach_component(&mut self, actor: ActorId, schema: impl Into<String>, data: BlackboardValue) -> Result<ComponentId, RuntimeError>;
    pub fn set_blackboard(&mut self, key: impl Into<String>, value: BlackboardValue);
    pub fn emit_event(&mut self, source: EventSource, payload: EventPayload);
    pub fn emit_presentation(&mut self, command: PresentationCommand);
    pub fn push_await(&mut self, token: AwaitToken);
    pub fn schedule_event(&mut self, due_tick: u64, source: EventSource, payload: EventPayload) -> DelayedEventId;
    pub fn cancel_delayed_event(&mut self, id: DelayedEventId);
}
```

Context 不提供 renderer/audio/backend、Editor widget、filesystem、network 或 thread handle。异步工作必须用 `AwaitToken` 回到固定 tick。

## Action Descriptor

Rust 类型是 schema 真源：

```rust
pub struct ActionDescriptor {
    pub id: String,
    pub input_schema: String,
    pub output_schema: String,
}

pub struct ActionInvocation {
    pub action_id: String,
    pub input: BTreeMap<String, BlackboardValue>,
}
```

内置 action 由 `astra.core` provider 注册。插件 action 由 loader 安装 host-side adapter：

```rust
world.register_action("provider.id", action);
world.unregister_action_provider("provider.id");
```

## FFI Action Flow

插件 descriptor 仍是作者输入 YAML。插件的 Rust 类型和 host DTO 才是 request/result 的 schema 真源。ABI 边界只传稳定值：

```rust
pub struct FfiActionRegistration {
    pub provider_id: RString,
    pub action_id: RString,
    pub input_schema: RString,
    pub output_schema: RString,
    pub invoke: extern "C" fn(RVec<u8>) -> RVec<u8>,
}
```

调用流程：

1. StateMachine 找到 action id，取 host registry 中的 `RuntimeAction` adapter。
2. Adapter 把 `ActionCallRequest` 用 postcard 编成 `RVec<u8>`。
3. 插件函数返回 postcard 编码的 `ActionCallResult`。
4. Adapter 解码 result，把 `ActionEffect` 按顺序应用到 `DeterministicActionContext`。
5. Runtime 记录 `ActionTrace`，成功后提交候选 mutation。

插件不能持有 `RuntimeWorld` 指针，也不能跨 ABI 保存 host 对象所有权。

## Effect List

Stage 1 支持的 effect 覆盖 EngineCore 范围：

| Effect | Host 行为 |
| --- | --- |
| `SetBlackboard` | 写入可序列化 Blackboard value |
| `CreateActor` | 创建 Actor，可把 stable id 写回 Blackboard |
| `AttachComponent` | 给 Actor 添加 schema/version/data component |
| `RemoveActor` / `DetachComponent` | 移除 Actor 或 Component |
| `EmitEvent` | 生成 RuntimeEvent，进入 EventQueue |
| `Presentation` | 追加 headless generic PresentationCommand |
| `Await` | 插入 AwaitToken |
| `ScheduleDelayedEvent` | 安排 fixed tick event |
| `CancelDelayedEvent` | 删除尚未触发的 delayed event |

## Delayed Events

`DelayedEventQueue` 保存 `ScheduledEvent { id, due_tick, sequence, source, payload }`。Runtime 每个 tick 先把到期事件按 `(due_tick, sequence, id)` drain 到 EventQueue，再统一按 EventQueue 顺序交给 StateMachine。

队列进入 `RuntimeSnapshot`，因此 save/load/replay 后 timer 类事件仍在同一 fixed tick 触发。

## Failure Policy

Action missing、payload decode 失败、插件返回 error、effect apply 失败都会生成 blocking diagnostic。Runtime 不 panic，不迁移当前 machine，继续执行其他 machine。Release profile 可以把 blocking diagnostic 映射成退出、错误页或 PIE pause。

## Verification

```bash
cargo test -p astra-runtime state_machine_tick
cargo test -p astra-runtime delayed_event
cargo test -p astra-plugin ffi_action_provider
cargo test -p astra-test native_smoke
```

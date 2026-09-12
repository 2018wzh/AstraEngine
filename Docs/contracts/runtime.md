# Runtime Contract

Runtime 使用 Actor/Component，按需安装 flat StateMachine。局部 ECS 可以优化批量 transform、粒子、sprite sorting 或音频 voice 更新，但不能进入 public save、Inspector 或脚本对象模型。

## Public API 草案

```rust
pub struct RuntimeWorld;
pub struct RuntimeConfig;
pub struct ActorId(pub StableId);
pub struct ComponentId(pub StableId);
pub struct TickInput { pub fixed_step: u64, pub delta_ns: u64, pub seed: u64 }
pub enum TickMode { Live, RestoreContinuation }
pub struct TickRequest { pub timing: TickInput, pub mode: TickMode, pub ingress: Vec<OrderedTickIngress> }
pub struct EngineModuleSlot(pub String);
pub struct ValidatedModuleBinding;
pub struct TickReport {
    pub step: u64,
    pub integrity_mode: TickIntegrityMode,
    pub diagnostics: Vec<Diagnostic>,
}

impl RuntimeWorld {
    pub fn create(config: RuntimeConfig) -> Result<Self, RuntimeError>;
    pub fn mount_module(&mut self, slot: EngineModuleSlot, binding: ValidatedModuleBinding) -> Result<(), RuntimeError>;
    pub fn tick(&mut self, request: TickRequest) -> Result<TickReport, RuntimeError>;
    pub fn register_action<A: RuntimeAction + 'static>(&mut self, provider_id: impl Into<String>, action: A) -> Result<(), RuntimeError>;
    pub fn unregister_action_provider(&mut self, provider_id: &str);
    pub fn schedule_event(&mut self, due_tick: u64, source: EventSource, payload: EventPayload) -> Result<DelayedEventId, RuntimeError>;
    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) -> Result<bool, RuntimeError>;
    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError>;
    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError>;
    pub fn debug_session(&self) -> RuntimeDebugSession<'_>;
}
```

`ValidatedModuleBinding` 只能由显式 registry selection、packaged eligibility、capability、package、target、profile、engine version、rustc/feature/ABI fingerprint 校验生成。上述 identity 由已验证的 `PackageHandle` 固化；重复 slot、token/slot 不一致或任一 identity 不一致必须在修改 world 前失败。`tick` 的首步固定为 `1`，之后每次只能递增 `1`；`seed` 必须等于 session seed，`delta_ns` 必须处于 `1..=1_000_000_000`。`PlayerInput` 与 `AwaitCompletion` 只能通过 non-zero、strictly increasing 的 `OrderedTickIngress` 提交。provider live output 不作为 Runtime ingress。load 后第一步必须使用一次 `RestoreContinuation`，之后回到 `Live`。重复、回退、跳步、非法 delta、seed mismatch、mode mismatch、ingress 乱序、缺少 required module 或任一 ingress 校验失败都返回稳定 blocking diagnostic，输入预检错误保持原状态，执行期失败处理见下文。

`runtime.world` 当前二进制 schema 为 `5.0.0`，外层产品 section 为 `astra.runtime.save_blob.v5`。v4 及更早布局和调用方请求的旧 minimum version 直接返回 `ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED`；不提供兼容 adapter 或迁移工具。

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
- 同一 machine 在单个 fixed tick 内连续执行 transition，直到稳定态、terminal state 或出现 blocking diagnostic。循环和 microstep 超限会回滚该 machine 在本 tick 的全部候选变更。
- Action 只通过 `DeterministicActionContext` 改 Actor/typed Component、Blackboard、EventQueue、AwaitToken、PresentationCommand 和 delayed event queue；实时路径不生成序列化 effect。
- Runtime action 在 host 内以 typed Rust action 注册。通用动态 action ABI 与 bytes invoke adapter 已删除；动态 gameplay provider 只通过 Provider ABI v3 返回 typed live/control output。
- `ActionRegistry` 拒绝空 descriptor、重复 action id 和 provider 冲突，不按后注册覆盖前注册。
- 状态机定义分双轨：引擎系统用 Rust code-first；项目 gameplay/VN 可以用 YAML/Graph 定义并 Cook 成 IR。
- Save 保存 `StableIdGenerator`、Actor/typed Component、StateMachine、Blackboard、AwaitQueue、完整 EventQueue、DelayedEventQueue 和 MutationLog，不保存实时 effect trace、ECS entity、native handle 或 Future 内部状态。typed component 只在 save 时编码，restore 后首次 typed read 才懒解码。

### Action ABI v2 与并行执行

每个 action 必须提交 `ActionDescriptor`，声明 `ActionExecutionClass`、确定性 read/write set 和 `stable_id_reservation`。空声明、动态访问、实际访问越权、pure action 写入、StableId 预留不足都会 fail closed。`RuntimeExecutorConfig` 只允许 `serial(1)` 或 `parallel(1..=8)`；Windows 与 Headless 产品入口显式选择 parallel，其他平台可选择 serial，序列化保存状态应一致。

StateMachine scheduler 先按稳定 machine id 构建 conflict DAG wave。wave 内任务只读同一不可变 Actor/Blackboard/event snapshot，分别生成 `ActorStoreDelta`、`BlackboardDelta` 和有序 effect；提交固定按 `(machine_id, microstep, action_index)` 验证。无法证明无冲突的 action 不会静默重试或降级，而是在注册或执行边界返回 blocking diagnostic。

每个 `StateMachineDefinition` 首次创建或反序列化后只编译一次 state→transition、terminal-state 与可证明的 event-kind dispatch index。tick event root 每 tick 只计算一次；每个 candidate 用稳定 consumed bitset 标记事件，不复制或 `Vec::remove` 事件队列。cycle fingerprint 组合缓存的 base Actor/Blackboard root、overlay delta metadata、event root 与 consumed ordinal，不重新序列化完整 ActorStore、component bytes 或剩余事件。event-kind dispatch 仍按原始 event sequence 选择 trigger，不能让 kind 排序改变消费顺序。

Runtime 外层 tick 原地执行；候选 action/machine 验证与整帧回滚的区别见下文。`TickIntegrityMode` 暂时保留现有输入诊断与 action 检查策略，tick 不生成 aggregate state/event/presentation digest。Shipping 对 ingress 顺序、delta 范围和缺少 required module 仍使用 WARN；Evidence 严格拒绝。这是后续任务生命周期迁移需要统一的现状，不代表两种模式具有相同的输入验证强度。

## 通用 Replay 移除

通用 Runtime replay recorder、transcript、checkpoint、Replay tick mode 和执行入口已删除，没有兼容 reader。原调用方使用正常 `save`/`load` 和后续物理输入继续运行；这不影响 VN backlog、语音重播和 CG/剧情回想。

World 不再保存或增量刷新 HistoryChain，也不再提供 aggregate `state_hash`/`event_hash`/`presentation_hash` API。`LoadReport` 返回恢复的 step 和 seed，不返回结构摘要。测试直接比较 typed snapshot 或存档字节，内容完整性由容器 hash 校验；旧摘要不再作验收基线，runtime.world v5 存档布局不变。诊断记录存储的进一步精简单独推进。

现有 `AwaitReplayPolicy` 命名仍待任务生命周期迁移：`RecordedResult` 接受 host 提交的完成结果，不能声明 timeout；`DeterministicTimeout` 必须声明 timeout step，并拒绝外部 completion。

## Delayed Event

`DelayedEventQueue` 用 `DelayedEventId` 标识任务，按 fixed tick 触发。Runtime 在每个 tick 开始阶段把到期事件按 `(due_tick, sequence, id)` 排序后进入 EventQueue，再交给 StateMachine。队列属于 `RuntimeSnapshot`，save/load 后必须保持同一触发 tick。

## 失败策略

Unknown event、invalid payload、missing required module、missing action、action failure、schema migration failure 都是 blocking diagnostic。Action failure 不迁移当前 machine，不提交候选 mutation，tick 继续处理其他 machine。PIE 可以暂停，packaged runtime 只能按 release profile 的 fatal policy 退出或进入安全错误页。

## Release Gate

`runtime.await.ordering`、`runtime.save_load`、`runtime.debug_snapshot`、`runtime.delayed_event`、`plugin.typed_runtime_provider` 是必需检查。普通测试检查具体状态、存档内容和错误；Runtime 不再为这些检查生成 aggregate digest。

## 嵌入式 World

`RuntimeWorld::create(RuntimeConfig)` 与 `create_with_integrity(config, mode)` 不需要产品包或 provider registry。宿主可直接创建 Actor、挂载与更新 typed Component、推进固定 tick 并存读档；未安装 StateMachine 的 World 使用同一执行与保存路径。

需要旧 packaged module binding 的产品宿主在首 tick 前调用 `with_package(PackageHandle)` 显式附加身份，且只能附加一次。`package_id()` 与 `package_handle()` 返回 Option，无包时不生成默认或伪造身份。无包 World 调用 packaged `mount_module` 返回 `ASTRA_RUNTIME_MODULE_PACKAGE_REQUIRED`；重复附加或首 tick 后附加返回 `ASTRA_RUNTIME_PACKAGE_LIFECYCLE`。普通 Actor/Component 操作不依赖这项绑定。

`runtime.world` v5 snapshot 将 package 身份改为 Option，NativeVN 外层 section 同步升级为 `astra.runtime.save_blob.v5`。旧布局明确拒绝；内存 typed state 与正常存读档仍由同一 RuntimeWorld 持有。权限来自宿主对 World 的所有权，诊断不记录组件 payload。验收包含无包/无 FSM 的 typed 更新和恢复、身份生命周期失败路径，以及现有 VN provider/Player 存档调用方。

## Tick 失败与恢复

RuntimeWorld tick 原地推进，不再为 Actor、Blackboard、Event、Await、delayed event 和 StateMachine 建立整帧撤销日志或状态 checkpoint。动作自身的访问声明与候选变更验证继续生效；已提交的其他 machine 工作不会因本 tick 后续错误撤回。

step、seed、mode 等输入预检在修改前完成；预检错误不终止 World。执行阶段返回错误、blocking/error diagnostic 或发生 Rust unwind panic 时，`is_failed()` 变为 true，首次调用返回根因；后续 tick、可变 World API、save 和 snapshot 返回 `ASTRA_RUNTIME_SESSION_FAILED`。`create_actor`、事件写入、移除/取消和 snapshot 等原先不可失败的 API 现返回 Result，产品调用方必须传播错误。DebugSession 与只读查询可用于诊断，动作 provider 注销仍可用于清理。

失败状态不进入正常存档。宿主可销毁 World，或明确读取已存在且通过格式验证的存档；读取失败保持当前失败状态，成功恢复后使用 RestoreContinuation 继续。宿主显式提供的 `restore_snapshot` 是同样的恢复边界。此调整不改变 runtime.world v5 二进制布局：删除的事务字段此前均未序列化。

验证覆盖多 machine 部分提交、动作访问错误、microstep 超限、Await policy 错误、panic、失败后的写入/保存拒绝以及从此前存档恢复。通用 replay 与 HistoryChain 已移除；诊断记录存储仍需后续精简。

## 宿主恢复验证

`load_with_validation(save, registry, validate)` 先验证容器并解码候选 RuntimeSnapshot，再由持有 World 的宿主检查和提取 typed 产品状态。闭包只修改候选 snapshot；返回错误时不替换当前 World，也不解除失败状态。验证成功后提交并进入 RestoreContinuation，同时返回宿主提取的数据。普通 `load`/`load_with_registry` 复用这一路径，不要求无产品 World 安装 VN 校验器。

NativeVN 在提交前检查外层 section 的 hash、v5 数字版本、package 身份、唯一 owner state component、component 版本、typed 解码与 VN state schema；验证失败保留 World、VN state 和待处理控制。成功时一起替换状态并清空旧控制。外层旧 v4 数字版本明确拒绝，需重新生成内部开发存档；嵌套 runtime.world v5 布局不变。验收包含合法 hash 下的无效 typed state、版本/包不符、失败 World 的拒绝恢复和正常恢复后续 tick。

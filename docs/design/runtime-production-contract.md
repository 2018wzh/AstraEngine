# Runtime Production Contract

状态：Production contract draft / not yet fully implemented  
定位：把 foundation `RuntimeWorld` 提升为可发布、可调试、可回放的 deterministic runtime core。本文是准 API 草案，用于指导 Phase 5+ 实现；字段和函数名可通过后续 ADR 调整，但实现不得重新发明边界。

## 1. 目标

Runtime production contract 必须保证：

- fixed-step runtime tick、variable presentation update 和 frame index 可确定重放。
- EventBus、Scheduler、StateMachine、Director 和 ControlPolicy 共享同一事件顺序模型。
- Actor/Component 生命周期、deferred destroy、preview attach 和 source patch proposal 有明确边界。
- Runtime 可被 CLI、Editor PIE、Runtime Debugger、MCP Runtime Host 和 packaged game 使用，但 Runtime 不依赖 Editor。
- 所有错误通过 diagnostics 输出，不以崩溃或隐式状态修复替代。

非目标：

- 不引入 UE `UObject`/GC/UHT。
- 不把 Editor widget、SDL/GPU/audio handle、ECS entity 或 C++ object ownership 暴露到 Runtime public API。
- 不让 Script、AI、Legacy VM 拥有主循环；它们通过 Scheduler、RuntimeEvent 和 provider contract 接入。

## 2. Tick And Event Ordering

Runtime tick 输入：

```yaml
schema: astra.runtime.tick_input.v1
frame_index: 120
fixed_step_index: 7200
delta_ns: 16666667
fixed_delta_ns: 16666667
input_events: []
package_profile: deterministic
debug_commands: []
```

Runtime tick 输出：

```yaml
schema: astra.runtime.frame_result.v1
frame_index: 120
fixed_steps_executed: 1
event_sequence_begin: 440
event_sequence_end: 457
scheduled_tasks_completed: []
presentation_commands: []
diagnostics: []
hashes:
  state: "..."
  events: "..."
  presentation: "..."
```

准接口：

```cpp
struct RuntimeTickInput;
struct RuntimeFrameResult;

class RuntimeWorld {
public:
    Result<RuntimeFrameResult> Tick(const RuntimeTickInput& input, DiagnosticSink& diagnostics);
    Result<void> QueueEvent(RuntimeEvent event, RuntimeEventMode mode, DiagnosticSink& diagnostics);
    RuntimeHashes Hashes() const;
};
```

排序规则：

- RuntimeEvent sequence 是全局单调递增值，不可由 provider 自行生成。
- fixed update 内按 `Immediate -> Queued -> Deferred -> Scheduler wake -> StateMachine action -> Presentation extraction` 的顺序处理。
- 相同 frame 内的 provider 输出必须带来源 event sequence，便于 replay mismatch 定位。
- variable presentation update 只生成 transient presentation state；需要保存的状态必须进入 Save/Replay contract。

## 3. Scheduler Contract

任务描述：

```yaml
schema: astra.runtime.scheduler_task.v1
task_id: task:/opening/typewriter
owner: actor:/systems/dialogue
created_sequence: 440
priority: normal
cancellation:
  policy: cancel_on_owner_destroy
wait:
  kind: event
  event_type: event:/dialogue.advance
save_policy: persistent
payload_schema: astra.vn.typewriter_task.v1
payload: {}
```

`WaitCondition.kind` 允许：

- `event`
- `time`
- `fixed_steps`
- `asset_ready`
- `script_resume`
- `debugger_resume`
- `provider_signal`

规则：

- Scheduler 只保存 task descriptor、wait condition、owner 和 deterministic payload；不保存线程、lambda、coroutine native frame 或 provider handle。
- provider 可注册 `provider_signal`，但 signal 必须转换为 RuntimeEvent 或 Scheduler wake record。
- task cancellation 必须产生 trace record；release profile 可将 dangling task 变为 blocking diagnostic。

## 4. Director And ControlPolicy

Director arbitration request：

```yaml
schema: astra.runtime.director_arbitration_request.v1
frame_index: 120
request_id: director:/opening/choice_lock
channel: dialogue
owner: actor:/systems/choice
requested_mode: exclusive
priority: story_critical
interrupt:
  allowed: false
conflicts:
  - actor:/systems/dialogue
```

结果：

```yaml
schema: astra.runtime.director_arbitration_result.v1
decision: allow
queued: false
blocking_owner: ""
diagnostics: []
```

规则：

- ControlPolicy 处理 owner/channel/local lock，Director 处理跨系统全局 phase、timeline lock、choice lock、AI permission window。
- `allow`、`queue`、`reject`、`interrupt` 必须可 replay。
- Editor debug override 必须标记为 `debug_command`，不能污染 packaged replay。

## 5. Actor Lifecycle

生产 Actor lifecycle：

```text
Declared -> Spawned -> Activated -> Deactivating -> Deactivated -> DestroyQueued -> Destroyed
```

规则：

- `Destroy()` 只进入 deferred destroy queue；实际移除发生在 frame boundary。
- preview attach 只存在于 EditorRuntimeSession overlay，不进入 packaged save。
- ActorHandle generation 必须阻止 stale handle reuse。
- Component activation 顺序由 descriptor dependency 明确声明；缺失依赖是 blocking diagnostic。
- source patch proposal 只写 canonical source，不直接从 PIE runtime object 反写 Content。

## 6. Diagnostics And Release Gate

必须定义稳定诊断码前缀：

- `ASTRA_RUNTIME_TICK_*`
- `ASTRA_RUNTIME_EVENT_*`
- `ASTRA_RUNTIME_SCHEDULER_*`
- `ASTRA_RUNTIME_DIRECTOR_*`
- `ASTRA_RUNTIME_LIFECYCLE_*`

Release Gate 检查：

- deterministic profile 下所有 persistent scheduler tasks 可序列化。
- Actor lifecycle 无 dangling owner、stale handle、deferred destroy leak。
- Director/ControlPolicy conflict 有可 replay 决策记录。
- Runtime public headers 不暴露 Editor/native handles。

## 7. Acceptance

`RuntimeStress` 必须覆盖：

- 1000+ Actor spawn/deferred destroy/snapshot/restore。
- 多状态机、多 Scheduler wait kind、多 ControlPolicy conflict。
- repeated replay state/event/presentation hash 一致。
- mismatch report 能定位到 frame、event sequence、actor/component 或 task。


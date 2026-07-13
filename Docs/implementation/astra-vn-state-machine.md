# AstraVN StateMachine Playback

本页定义 AstraVN 如何用 EngineCore `StateMachine` 推进 `CompiledStory`。实现位于 `astra-vn-core` 和 `astra-vn-runtime-provider`；`astra-vn` 只保留 facade 与 re-export。

## Runtime Shape

每个运行中的 VN instance 创建一个故事推进 `StateMachine`。角色、背景、文本窗、视频层和系统页不是状态机单位，它们是 Actor/Component、VN Core state、presentation state 或 Luau policy state。

Runtime `StateMachine` 保持 flat FSM。VN 的路线、call/return、system story 和 savepoint stack 放在可序列化 `VnRuntimeState`，不把层级状态机、并行状态机或 pushdown stack 上提到 EngineCore。

```text
.astra source
  -> CompiledStory
  -> VnRuntimeState + VnCommandCursor
  -> Runtime StateMachine transition
  -> astra.vn.step action
  -> VnStepOutput
  -> RuntimeEvent / PresentationCommand / AudioCommand / AwaitToken
```

状态机只负责 fixed tick 边界上的剧情推进。Director、Renderer2D、AudioGraph 和 FilterGraph 只消费可序列化 command，并用 `AwaitResult` 或 diagnostic 回到 Runtime。

## Runtime Types

Rust 类型是 schema 真源。当前 `VnRuntimeState` 使用结构化 cursor、call stack 和独立 system stack；不再保存互相分离的 story/state/usize cursor。

```rust
pub struct VnRuntimeState {
    pub schema: String,
    pub instance_id: String,
    pub profile: String,
    pub locale: String,
    pub cursor: Option<VnCommandCursor>,
    pub call_stack: Vec<VnCallFrame>,
    pub system_stack: Vec<VnSystemFrame>,
    pub system: VnSystemState,
    pub pending_choice: Option<PendingChoice>,
    pub variables: BTreeMap<String, BTreeMap<String, i64>>,
    pub backlog: Vec<BacklogEntry>,
    pub read_state: BTreeSet<String>,
    pub voice_replay: BTreeMap<String, VoiceReplayEntry>,
    pub route_coverage: BTreeSet<String>,
    pub route_flags: BTreeMap<String, VnRouteFlag>,
    pub pending_wait: Option<VnWaitState>,
}

pub struct VnCommandCursor {
    pub story_id: String,
    pub state_id: String,
    pub scene_id: String,
    pub command_id: String,
    pub ordinal: usize,
}

pub struct VnWaitState {
    pub kind: VnWaitKind,
    pub fence: String,
    pub command_id: String,
    pub await_id: Option<String>,
}

pub struct VnCallFrame {
    pub return_to: VnCommandCursor,
    pub source_command_id: String,
    pub reason: String,
}

pub struct VnStepOutput {
    pub next_cursor: Option<VnCommandCursor>,
    pub wait: Option<VnWaitState>,
    pub events: Vec<VnEvent>,
    pub presentation: Vec<PresentationCommand>,
    pub audio: Vec<VnAudioCommand>,
    pub awaits: Vec<String>,
    pub timeline_tasks: Vec<VnTimelineTask>,
    pub mutations: Vec<VnMutationRecord>,
}
```

`VnRuntimeState` 和 policy state 作为 typed component 进入完整 Runtime snapshot。NativeVN product provider 把自描述 Runtime save container 封装成唯一 `runtime.world`/`astra.runtime.save_blob.v2` section，不再并列保存容易漂移的 `vn.runtime_state`/`vn.policy_state` 权威副本。Provider 把 `VnStepOutput.awaits` 映射成 Runtime `AwaitToken`，把 audio/timeline DTO 写成 hash-validated `SerializedEffectEnvelope`。Luau snapshot 只能保存策略私有的可序列化值，不能保存 function、thread、userdata、native handle 或 coroutine state。

## Step Action

`astra.vn.step` 是 Stage 3 的主 action。NativeVN session 创建一个 RuntimeWorld、VN Actor、VN/policy typed component 和自循环 flat StateMachine。Action 从 trigger event 解码 `VnPlayerCommand`，调用无隐藏 session 状态的 reducer，再通过 `DeterministicActionContext` 替换 VN component、写 mutation record、发 RuntimeEvent/PresentationCommand/AwaitToken 和 audio/timeline effect。

```text
StateMachine state: vn.running
guard: EventIs { kind: "vn.tick" | "player.advance" | "choice.selected" | "await.completed" }
actions:
  - astra.vn.step
```

`astra.vn.step` 的提交规则：

- 普通 presentation command 可以连续执行，输出 `PresentationCommand`、`AudioCommand` 和 mutation trace。
- `text` 必须写 backlog、read-state 和 voice replay，再进入 `Dialogue` wait。
- `choice` 必须建立 savepoint、输出选择 UI，并进入 `Choice` wait。
- `wait`、`movie end:wait` 和 voice sync 必须落成 `AwaitToken` 或 `Fence`。
- `jump`、`call`、`return` 只修改 `VnCommandCursor` 和 `VnCallFrame`，不改 EngineCore 状态机结构。
- `system_page` push `system_stack`，返回时恢复 `return_to`。

任一 action failure 由 EngineCore 回滚该 machine 在本 tick 的全部候选 mutation，当前 Runtime `StateMachine` 不迁移。run-to-quiescence 的循环或 microstep 超限也按同一事务边界阻断。VN Core 把 command id、source ref 和 wait kind 放进 diagnostic，供 PIE 和 Release Gate 定位。

## Trigger Event

VN step action 通过 `DeterministicActionContext` 的只读 trigger event 接口读取 payload：

```rust
impl DeterministicActionContext<'_> {
    pub fn trigger_event(&self) -> Option<&RuntimeEvent>;
}

pub struct ActionCallRequest {
    pub step: u64,
    pub action_id: String,
    pub trigger_event: Option<RuntimeEvent>,
    pub input: BTreeMap<String, BlackboardValue>,
}
```

Host action 和 FFI action 都读取同一个 `trigger_event`。`choice.selected`、`player.advance` 和 `await.completed` 的 payload 不先写进全局 Blackboard；`astra.vn.step` 直接消费触发事件并输出可记录 mutation。这样 replay 只依赖 ordered event trace，不依赖临时全局键。

## Command Flow

Dialogue flow：

```text
vn.running + vn.tick
  -> astra.vn.step
  -> execute stage/show/camera/voice
  -> execute text(line.hello)
  -> push backlog/read-state/voice replay
  -> emit TextWindow presentation
  -> create dialogue AwaitToken
  -> wait = Dialogue(line.hello)

vn.running + player.advance
  -> astra.vn.step
  -> resolve Dialogue wait
  -> continue cursor until next wait
```

Choice flow：

```text
choice command
  -> emit choice presentation
  -> create choice AwaitToken
  -> wait = Choice(choice.where)

choice.selected(choice.library)
  -> commit selected option
  -> set route flag
  -> cursor = state.library first command
```

System page flow：

```text
system_page kind:save
  -> capture savepoint
  -> push SystemPage wait
  -> call system story

system_page.return
  -> restore return_to cursor
  -> continue story
```

## Timeline And Director

Timeline 是 Director 的输入，不是剧情状态。`PresentationTimeline`、timeline track 和 Editor metadata 必须绑定 `command_id`、source span 和 rollback scope。`join_policy` 决定 VN 是否进入 `Fence` wait；`cancel` 和 `skip_policy` 只改变 presentation/audio completion path，不改变 route、backlog 或 read-state。

Voice sync 由 `TextWindowState.voice_replay`、AudioGraph voice channel 和 `Fence` 共同完成。Movie end 通过 `AwaitToken` 回到 fixed tick。Renderer 或 Audio provider 不支持某个 effect 时，fallback 只能替换 presentation/audio effect，不能推进或回退 VN cursor。

## Luau Policy Boundary

Luau policy 可以在 `before_command`、`after_command`、`tick`、`render_frame` 和 `audio_frame` 中请求 presentation、audio、timeline 和 mutation，但所有权威写入都必须进入 `VnStepOutput` 或记录型 `astra.mutate`。直接修改 Luau table 只影响策略私有缓存，不影响 save、replay、backlog、read-state 或 route flags。

多个策略包提供同一 command 或 preset 时，项目 manifest 必须显式绑定 provider。绑定结果进入 package metadata 和 release report。

## Test Mapping

`T-S3-CORE-01` 必须证明 command cursor、dialogue wait、choice payload、route flag 和 call/return stack 都由 `VnRuntimeState` 驱动。

`T-S3-PRESENT-01` 必须证明 presentation、timeline、voice fence 和 movie await 从 `VnStepOutput` 进入 deterministic state/event/presentation hash。

`T-S3-EDIT-01` 必须证明 Graph/Timeline 修改仍回写同一 `command_id`，并保留 wait/fence source map identity。

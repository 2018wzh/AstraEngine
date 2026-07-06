# AstraVN StateMachine Playback

本页定义 AstraVN 如何用 EngineCore `StateMachine` 推进 `CompiledStory`。本页是 implementation blueprint；`Engine/Source/Runtime/astra-vn` 仍是后续实现目标，不表示当前 crate 已存在。

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

## Planned Types

Rust 类型是 schema 真源；下面字段是 Stage 3 实现必须保留的语义边界。

```rust
pub struct VnRuntimeState {
    pub instance_id: VnInstanceId,
    pub story_id: StoryId,
    pub cursor: VnCommandCursor,
    pub wait: Option<VnWaitState>,
    pub call_stack: Vec<VnCallFrame>,
    pub system_stack: Vec<VnCallFrame>,
    pub route_flags: RouteFlagSet,
    pub variables: VnVariableStore,
    pub backlog: BacklogState,
    pub read_state: ReadState,
    pub voice_replay: VoiceReplayIndex,
}

pub struct VnCommandCursor {
    pub state_id: StateId,
    pub scene_id: SceneId,
    pub command_id: CommandId,
    pub ordinal: u32,
}

pub enum VnWaitState {
    Dialogue { command_id: CommandId, await_id: AwaitTokenId },
    Choice { command_id: CommandId, await_id: AwaitTokenId },
    Fence { command_id: CommandId, fence_id: FenceId },
    Movie { command_id: CommandId, await_id: AwaitTokenId },
    SystemPage { command_id: CommandId, return_to: VnCommandCursor },
}

pub struct VnCallFrame {
    pub return_to: VnCommandCursor,
    pub source_command_id: CommandId,
}

pub struct VnStepOutput {
    pub next_cursor: VnCommandCursor,
    pub wait: Option<VnWaitState>,
    pub events: Vec<RuntimeEvent>,
    pub presentation: Vec<PresentationCommand>,
    pub audio: Vec<AudioCommand>,
    pub awaits: Vec<AwaitToken>,
    pub timeline_tasks: Vec<PresentationTimeline>,
    pub mutations: Vec<VnMutationRecord>,
}
```

`VnRuntimeState` 进入 save section 和 replay hash。Luau snapshot 只能保存策略私有的可序列化值，不能保存 function、thread、userdata、native handle 或 coroutine state。

## Step Action

`astra.vn.step` 是 Stage 3 的主 action。它读取 `VnRuntimeState`、`CompiledStory` 和触发事件，在同一个 tick 内连续执行非等待 command；遇到 dialogue、choice、wait、movie end、system page 或 timeline fence 时提交 `VnWaitState` 并停止。

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

任一 action failure 由 EngineCore 回滚候选 mutation，当前 Runtime `StateMachine` 不迁移。VN Core 需要把 command id、source ref 和 wait kind 放进 diagnostic，方便 PIE 和 Release Gate 定位。

## Trigger Event

VN step action 需要读取触发事件 payload。Stage 1 的 action context 只暴露 mutation 入口，因此 Stage 3 需要给 `DeterministicActionContext` 增加只读接口：

```rust
impl DeterministicActionContext<'_> {
    pub fn trigger_event(&self) -> &RuntimeEvent;
}

pub struct ActionCallRequest {
    pub step: u64,
    pub action_id: String,
    pub trigger_event: RuntimeEvent,
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

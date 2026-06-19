# Runtime Core 设计

状态：Target Architecture  
定位：Astra runtime 的确定性执行、事件、调度、状态机、Director、存档和回放中心。

## 1. 目标

Runtime Core 必须把 headless foundation 提升为可发布 2D/VN runtime：

- 相同 package、seed、输入和 committed AI output 必须产生相同 state hash、event hash 和 presentation hash。
- Runtime 可脱离 Editor 运行、暂停、单步、保存、加载、回放、诊断和 profiling。
- Editor、CLI、MCP、Script 和插件只能通过 public DTO、handle、event、command 和 inspector/debugger API 观察或驱动 Runtime。
- Runtime 不依赖 Editor UI、MCP server implementation、AI provider implementation、renderer/audio native handle 或 legacy VM 内部结构。

## 2. RuntimeWorld

`RuntimeWorld` 是组合 facade，不是全局单例：

```text
RuntimeWorld
├─ SceneWorld / ActorWorld
├─ RuntimeEventBus
├─ StateMachineRuntime
├─ RuntimeScheduler
├─ BlackboardStore
├─ ControlPolicySystem
├─ Director
├─ PresentationExtractor
├─ SaveReplayService
└─ RuntimeDiagnostics
```

创建输入：

- package/project manifest
- runtime config
- module policy
- initial scene/script entry
- deterministic seed
- optional replay input stream

销毁顺序：

```text
Stop accepting input
  -> drain or cancel scheduler tasks
  -> flush event trace
  -> snapshot or discard runtime state
  -> deactivate actors/components
  -> release script/media/resource handles
  -> shutdown services
```

## 3. Tick Model

Runtime 分为 fixed update 和 presentation update：

```text
Frame N
  -> Collect external input
  -> Append deterministic input events
  -> FixedUpdate one or more steps
  -> Drain queued runtime events by sequence
  -> Step scheduler / timers / state machines
  -> Extract presentation commands
  -> Variable presentation update
  -> Hash state/event/presentation
```

规则：

- `frame_index`、`fixed_step_index`、`event_sequence` 单调递增并进入 replay。
- Timer 使用 game time 或 fixed time，不使用 wall clock 决定剧情状态。
- Runtime 可 pause、single step、resume；pause 不推进 game time，但允许 debugger 和 inspector。
- Variable presentation update 不改变 gameplay/runtime state，只更新可丢弃的插值、capture 或 overlay。

## 4. RuntimeEvent Contract

事件最小 schema：

```yaml
event_id: event:/runtime/00001234
type: astra.vn.dialogue.say_requested
category: story
sequence: 1234
frame_index: 88
source:
  kind: script
  id: native:/Scripts/opening
target:
  kind: actor
  id: actor:/story/dialogue_system
payload_schema: astra.vn.say_requested.v1
payload:
  speaker: actor:/characters/alice
  text_key: loc:/opening/alice_001
trace:
  script_location: Scripts/opening.astra:12
  audit_ref: null
```

EventBus modes：

- `immediate`：当前处理栈内同步执行，仅限 internal guard/action。
- `queued`：按 priority、sequence、target order 执行，是默认模式。
- `deferred`：下一 fixed step 执行。
- `scheduled`：由 scheduler/timer 唤醒。

错误策略：

- unknown event type：blocking diagnostic in validate/cook；runtime fallback 为 drop with trace。
- invalid payload schema：blocking diagnostic；PIE 可进入 paused failure state。
- missing target actor：soft reference diagnostic；根据 event policy 选择 drop、retry、fallback actor。
- recursive event storm：RuntimeDiagnostics 记录 source chain，超过阈值后暂停 PIE 或触发 fatal runtime policy。

## 5. Scheduler And Timers

Scheduler 任务必须可取消、可快照、可迁移：

```text
TaskId
owner ActorId / SystemId
state pending/running/waiting/cancelled/completed
wait condition event/time/asset/script/debugger
serialized continuation token
timeout policy
```

禁止：

- 在 save-safe task 中捕获 C++ pointer、native handle、thread local state。
- 用 wall clock、thread scheduling order 或 provider response time 决定 deterministic state。

验收：

- 保存后恢复，等待事件、等待时间、等待资产和等待脚本状态继续一致。
- 任务取消、Actor destroy、scene unload 必须释放等待项并产生 trace。

## 6. StateMachine Runtime

状态机定义是 asset/source，实例是 Actor-bound component state：

```yaml
id: native:/StateMachines/DialogueBox
schema: astra.statemachine.v1
states:
  idle:
    on:
      astra.vn.dialogue.say_requested: typing
  typing:
    enter:
      - command: presentation.text.start_typewriter
    on:
      astra.presentation.typewriter.finished: waiting_input
snapshot:
  save_current_state: true
  save_delayed_events: true
  save_timers: true
```

Guard/action 规则：

- Guard 只读取 event payload、Blackboard、Actor snapshot、Director state，不产生 side effect。
- Action 可 emit event、set Blackboard、request presentation、schedule task，但必须进入 trace。
- Hot reload 必须验证 old state 到 new state 的 migration；失败则 rollback。

## 7. Director And ControlPolicy

Director 负责全局冲突仲裁：

- story phase
- timeline lock
- choice lock
- player input window
- runtime AI permission window
- route/canon constraints
- emergency stop and fallback

ControlPolicy 负责 Actor/channel 级控制权：

```yaml
owner: actor:/characters/alice
channels:
  pose:
    owner: story_script
    lock: timeline:/opening
    interrupt: reject_lower_priority
  dialogue:
    owner: dialogue_system
    interrupt: queue
priority:
  system: 100
  critical_timeline: 90
  story_script: 80
  player_choice: 70
  runtime_ai: 40
```

Runtime AI、Script、Timeline 和 player input 的冲突都必须通过 Director/ControlPolicy，不允许直接改 Actor state。

## 8. Save / Load / Replay

Save container：

```yaml
header:
  engine_version: 0.1.0
  package_hash: sha256:...
  save_schema: astra.save.v1
  created_frame: 1200
versions:
  project: 1
  modules:
    astra.vn: 1
sections:
  world: offset/size/hash
  actors: offset/size/hash
  components: offset/size/hash
  state_machines: offset/size/hash
  scheduler: offset/size/hash
  script: offset/size/hash
  director: offset/size/hash
  resources: offset/size/hash
  ai_committed_output: offset/size/hash
  extension_state: offset/size/hash
```

Replay stream：

- external input
- choice selections
- script decisions
- committed AI output
- runtime events hash
- presentation commands hash
- state hash checkpoints

Load policy：

- compatible schema：migrate then load。
- unknown optional module state：preserve opaque extension blob if module absent; warn。
- missing required module：blocking diagnostic。
- hash mismatch：reject load unless explicit debug override。

## 9. Diagnostics And Debugging

RuntimeDiagnostics 必须输出 machine-readable diagnostics：

- category：runtime/event/scheduler/state_machine/save/replay/director/resource。
- severity：info/warning/error/blocking/fatal。
- object refs：ActorId、ComponentId、EventId、TaskId、AssetId、Script location。
- suggested fix：用于 Editor、CLI、Copilot MCP。

Debugger API：

- inspect world/actor/component/state machine。
- pause/resume/single-step。
- inspect event queue and scheduler tasks。
- capture save snapshot。
- run replay comparison。
- request source patch proposal for PIE transient changes。

## 10. 验收

- 两次运行同一 package、seed、input 和 committed AI output，state/event/presentation hash 一致。
- 1000+ Actor、多状态机、多 task、多资源等待可稳定 tick、save、load、replay。
- Replay mismatch 能定位 frame、event、actor/component、script command 或 presentation command。
- Editor PIE 与 packaged runtime 共享同一 `RuntimeWorld` 路径。
- Runtime public API 不暴露 Editor widget、C++ Actor pointer、ECS entity、SDL/GPU/audio native handle。



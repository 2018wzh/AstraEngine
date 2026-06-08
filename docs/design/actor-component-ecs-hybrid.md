# Actor/Component、状态机与局部 ECS 设计

状态：Target Architecture  
关联 ADR：`../adr/0010-actor-component-statemachine-core.md`

## 1. 目标

AstraEngine 的公开运行时对象模型是 Actor/Component。状态机是 Component 的一类，是叙事、演出、UI、交互和 AI Intent 执行的核心组织方式。

ECS / Data-Oriented system pack 只用于性能热点或批量数据处理，例如粒子、大批量动画、渲染批处理、legacy timeline 推进和 filter 执行调度。ECS 不是公开 authoring model，也不是唯一权威运行时中心。

## 2. 分层

```text
Core
基础类型、ModuleManager、ServiceRegistry、PropertySystem、Serialization

Scene
World、Scene、Actor、Component、Prefab、Lifecycle、ActorHandle

Runtime
EventBus、StateMachineRuntime、Blackboard、ControlPolicy、Director、Save/Replay

Presentation
Timeline、UI、Camera、Animation、PresentationCommand、FilterProfile

AstraVN
BackgroundActor、CharacterActor、DialogueSystemActor、ChoiceSystemActor、AudioSystemActor
VN Event、VN StateMachine、VN DSL / Graph

Local ECS / Data-Oriented Packs
性能热点内部实现，不暴露给脚本、Editor、MCP 或动态模块 ABI
```

## 3. Actor 公共模型

Actor 必须具备：

- `ActorId`：稳定、可存档、可回放。
- `ActorTypeId`：稳定类型 ID，例如 `astra.vn.character`。
- `ActorName`：编辑器和调试显示名，不作为唯一身份。
- Transform：2D 默认使用 `Transform2DComponent`。
- Component 集合：由 `ComponentDescriptor` 和序列化数据描述。
- Lifecycle：spawn、activate、deactivate、destroy、save、restore、preview attach。
- `ActorHandle`：受控 handle，不暴露 C++ 指针或所有权。

公开接口传递 DTO、handle、descriptor、snapshot、event 或 command。动态模块不得跨 ABI 传递 C++ `Actor*`。

### 3.1 Creator-facing Authoring

Editor 必须向创作者提供：

- Actor Type Palette：按模块、类别、tag 搜索可创建 Actor。
- Component Palette：显示可挂载组件、依赖、互斥关系和默认值。
- Defaults Panel：编辑 actor/component 默认值，写入 prefab 或 source object。
- Prefab / Variant Browser：创建 base prefab、variant、override 和 revert。
- Inspector Metadata：display name、category、tooltip、validation、requires review、read-only。
- Preview Attach：在不写入正式 scene 的情况下预览 Actor、Component 或 Presentation 状态。

组件可编辑性分级：

- Creator Editable：Transform2D、Tag、SpritePresentation、DialogueText、ChoiceList、AudioCue、Camera、Timeline、FilterProfile。
- Review Required：AI-editable text、character profile、route-critical choice、release-sensitive metadata。
- Runtime Managed：Lifetime runtime state、StateMachine current state、queued events、ControlPolicy lock、ECS internal data。
- System Only：native handle、ECS entity、renderer/audio resource instance、module private state。

Authoring 状态流：

```text
Palette Selection -> Actor Draft -> Inspector Edit -> Validate -> Scene Source
Prefab Base -> Variant Override -> Preview -> Apply/Revert
PIE Runtime Snapshot -> Debug Inspect -> Optional Source Patch
```

Component inspector metadata 示例：

```yaml
type_id: astra.vn.character_profile
display_name: Character Profile
category: VN/Character
properties:
  - id: display_name
    label: Display Name
    editor: localized_text
    order: 10
    flags: [creator_editable]
  - id: route_role
    label: Route Role
    editor: enum
    order: 20
    flags: [requires_review]
  - id: current_emotion
    label: Current Emotion
    editor: readonly_badge
    order: 30
    flags: [runtime_only, read_only]
validation:
  - rule: display_name.required
  - rule: route_role.review_for_release
```

Prefab/variant source 最小字段：

```yaml
id: native:/Prefabs/Characters/Alice
actor_type: astra.vn.character
base: null
components:
  astra.transform2d:
    position: [0, 0]
  astra.vn.character_profile:
    display_name: loc:/characters/alice/name
variants:
  - id: native:/Prefabs/Characters/AliceSchool
    overrides:
      astra.vn.character_profile.costume: school
```

## 4. Component 类型

基础组件：

- `Transform2DComponent`
- `BlackboardComponent`
- `ControlPolicyComponent`
- `StateMachineComponent`
- `LifetimeComponent`
- `TagComponent`

Presentation 组件：

- `SpritePresentationComponent`
- `BackgroundPresentationComponent`
- `DialogueTextComponent`
- `ChoiceListComponent`
- `AudioCueComponent`
- `CameraComponent`
- `TimelineComponent`
- `FilterProfileComponent`

VN 组件：

- `CharacterProfileComponent`
- `EmotionComponent`
- `DialogueParticipantComponent`
- `CharacterVisualBackendComponent`

Component 优先是数据和 schema，不直接持有全局服务。行为通过状态机、事件、脚本 API、system pack 或服务扩展表达。

## 5. 状态机组件

每个 Actor 可拥有多个状态机。原则是：

```text
谁拥有这个状态，状态机就绑定在谁身上。
```

示例：

```text
CharacterActor Alice
├─ CharacterPresentationSM
├─ EmotionSM
├─ AnimationSM
├─ DialogueSM
├─ InteractionSM
└─ AIBehaviorSM

DialogueSystemActor
└─ DialogueBoxSM

ChoiceSystemActor
└─ ChoiceSM

SceneActor
└─ BackgroundSM
```

状态机通过 EventBus 收发事件，通过 Blackboard 共享上下文，通过 ControlPolicy 决定事件是否执行、排队、拒绝或打断。

## 6. ControlPolicy 与 Director

多 Actor、多状态机、Timeline、玩家输入、AI 和 legacy VM 会产生控制权冲突。每个可控 Actor 应挂载 `ControlPolicyComponent`：

```text
ControlPolicyComponent
├─ current_owner
├─ locked_channels
├─ interrupt_policy
├─ queued_events
├─ ai_allowed
└─ priority_table
```

默认优先级：

```text
System
  > Critical Timeline
  > Story Script
  > Player Choice
  > Player Interaction
  > AI Intent
  > Ambient Behavior
```

`StoryDirectorActor` 或 `DirectorService` 负责全局剧情阶段、路线、Timeline lock、AI 可用区间和 legacy VM 同步。

## 7. 局部 ECS 使用规则

可以使用 ECS 的场景：

- 粒子、批量 sprite 动画、路径点、碰撞热点扫描。
- FilterGraph pass 调度。
- Legacy timeline / score 高帧率推进。
- 大量短生命周期特效和音频请求。

不可使用 ECS 作为：

- 脚本、Editor、MCP 或插件公开 API。
- 稳定存档 ID。
- Actor/Component authoring model 的替代物。

局部 ECS 与 Actor 世界之间通过稳定 `ActorId`、snapshot、event 和 service DTO 同步。

## 8. 存档

存档必须使用 ActorId 和 Component data，不保存 ECS entity 原始值或 C++ 指针。状态机保存当前状态、延迟事件、计时器、Blackboard 引用和 ControlPolicy lock。

验收：

- 创作者可从 palette 创建 Character、DialogueSystem、ChoiceSystem、Camera，并通过 Inspector 配置。
- Runtime-managed 字段在 Inspector 中可读但不可直接写入 source。
- PIE 中的 runtime 改动必须明确区分为 preview state、debug command 或 source patch proposal。

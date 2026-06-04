# 总体架构设计

## 1. 项目定位

AstraEngine 是高度可定制化、模块化的 2D 引擎。它以视觉小说和互动叙事为第一落地场景，但 Core 面向更广的 2D 叙事、演出和轻量玩法：

- 传统视觉小说、ADV、互动小说。
- 动态漫画、动态绘本、点击解谜、养成、轻 RPG、回合制和卡牌。
- AI 协作式创作与运行时受控内容生成。
- 旧 VN 引擎模拟器、兼容运行与现代化表现。

非目标：

- 不以复杂 3D 动作、FPS、高实时网络竞技或大型开放世界为主目标。
- 不引入完整 UE `UObject`、UHT、GC 或跨 ABI C++ Actor 继承体系。
- 不让 AI、Live2D、Lua 或旧 VM 污染 Core。

## 2. 顶层分层

```text
AstraEngine
├─ Core
│  ├─ Foundation
│  ├─ ModuleManager
│  ├─ ServiceRegistry
│  ├─ PropertySystem
│  └─ Serialization
├─ Platform
│  ├─ Window / Input / FileSystem
│  ├─ Thread / Timer
│  └─ DynamicLibrary
├─ Asset
│  ├─ AssetId / ResourceHandle
│  ├─ VFS / PackageReader
│  ├─ AssetRegistry
│  ├─ Importer / Cooker
│  └─ HotReload
├─ Media
│  ├─ RHI / Renderer2D
│  ├─ RenderGraph / FilterGraph
│  ├─ Text / Font
│  ├─ Audio / Video
│  └─ Animation
├─ Scene
│  ├─ World / Scene
│  ├─ Actor / Component
│  ├─ Prefab / Defaults
│  └─ Local ECS / Data-Oriented Packs
├─ Runtime
│  ├─ EventBus
│  ├─ StateMachineRuntime
│  ├─ Task / Coroutine Scheduler
│  ├─ Blackboard
│  ├─ ControlPolicy / Director
│  └─ Save / Load / Replay
├─ Script
│  ├─ ScriptRuntimeHost
│  ├─ Native Script Runtime
│  ├─ Lua Runtime
│  ├─ Legacy VM Runtime
│  └─ Debug / Event Bridge
├─ Presentation
│  ├─ Timeline
│  ├─ UI / Camera / Effects
│  ├─ Presentation Command
│  └─ Presentation Libraries
├─ AstraVN
│  ├─ VN DSL / Graph
│  ├─ Dialogue / Choice
│  ├─ Character / Background / Audio Cue
│  └─ VN State Machines
├─ AI
│  ├─ Editor Collaboration
│  ├─ Runtime Intent
│  ├─ Provider Modules
│  └─ Agent Audit
├─ Compat
│  ├─ Legacy Package Readers
│  ├─ Legacy VM / Script Runtime
│  ├─ API Mapper
│  ├─ Modernization Profile
│  └─ Compatibility Inspector
└─ Editor
   ├─ Script / Graph / Timeline Editors
   ├─ Scene / Asset / Inspector
   ├─ AI Workbench / Review Queue
   └─ Runtime Debugger
```

## 3. 核心运行链路

创作与运行的主要链路分三层：

```text
Creator DSL / Graph / Legacy VM / AI Intent
  -> Runtime Event
  -> Actor-bound StateMachine
  -> Presentation Command
  -> Scene / Media / Asset / Audio / FilterGraph
```

示例：

```text
say alice "早上好。"
  -> VN.SayRequested
  -> DialogueSystemActor.DialogueBoxSM
  -> Alice.DialogueSM + Alice.AnimationSM
  -> CreateTextBox / StartTypewriter / PlayVoice
```

`RuntimeCommand` 可作为日志、回放和兼容适配的记录格式存在，但不是唯一运行时中心。核心中心是 Actor、EventBus、StateMachineRuntime 和 Presentation Command。

## 4. Core 与 VN 边界

Core 负责：

- 基础类型、日志、错误、配置、时间、路径。
- ModuleManager、ServiceRegistry、ExtensionRegistry。
- PropertySystem、TypeId、PropertyId、schema generation。
- 序列化、版本迁移和 diagnostics。

Runtime/Scene 负责：

- Actor、Component、World、Scene、StateMachineRuntime、EventBus。
- ControlPolicy、Blackboard、Task/Coroutine、Save/Replay。

AstraVN 负责：

- VN DSL、VN Event、VN Graph。
- Dialogue、Choice、Character、Background、Audio cue。
- 预定义 VN 状态机和 VN Presentation Library。

Core 不负责：

- VN 剧情语义、Live2D、AI 生成、旧 VN opcode、具体模型 Provider、编辑器 UI。

## 5. Actor、状态机与 Director

Actor 是公开运行时对象模型。每个 Actor 可挂载普通 Component 和 StateMachineComponent。

```text
CharacterActor Alice
├─ TransformComponent
├─ BlackboardComponent
├─ ControlPolicyComponent
├─ CharacterPresentationSM
├─ EmotionSM
├─ DialogueSM
├─ AnimationSM
└─ AIBehaviorSM
```

多状态机协作通过以下机制完成：

- `EventBus`：分发 RuntimeEvent、VNEvent、PresentationEvent。
- `Blackboard`：共享角色、场景或系统上下文。
- `ControlPolicy`：判断控制权、优先级、打断、排队或拒绝。
- `Director`：负责全局叙事仲裁、Timeline lock 和剧情阶段约束。

## 6. Script Runtime

`ScriptRuntimeHost` 管理多个同级脚本运行时：

```text
ScriptRuntimeHost
├─ AstraNativeRuntime
├─ LuaRuntime
├─ BGIRuntime
├─ KirikiriRuntime
└─ CustomRuntime
```

脚本运行时只能通过 Script API、RuntimeEvent 和 Presentation API 影响世界。旧 VM 不需要反编译为 Astra DSL；它可以保存自己的 VM 状态并通过 API Mapper 输出事件。

## 7. Media 与 FilterGraph

`Media` 提供 2D 表现基础。FilterGraph 是统一后处理和现代化管线：

```text
Layer Render
  -> Per-Layer FilterGraph
  -> Composite
  -> Native UI / Text
  -> Final Screen FilterGraph
  -> Present
```

旧游戏现代化必须尽量 layer-aware：背景、角色、UI、文本、最终画面分别处理。文本优先重新排版，不做截图式超分。

## 8. 插件与模块

项目级扩展默认使用动态模块。模块通过 C ABI 进入，使用 ServiceRegistry 和 ExtensionRegistry 注册能力：

- Actor type、Component descriptor、StateMachine type。
- Script runtime、Script API provider。
- Filter、Renderer pass、Asset importer、Cook processor。
- AI Provider、Runtime Intent validator、Agent audit sink。
- Legacy package reader、VM runtime、API mapper、modernization profile。
- Editor panel、MCP resource/tool/prompt。

跨 ABI 不传递 STL ownership、C++ Actor 指针、renderer/audio native handle 或 Editor widget。

## 9. 存档与回放

存档不能只保存 label 和变量。必须保存：

- World、Scene、Actor 和 Component 状态。
- 所有 StateMachine 当前状态和延迟事件。
- Blackboard、ControlPolicy lock、Director 状态。
- Script Runtime 状态。
- AI 已提交输出和 committed intent。
- FilterProfile、Timeline、资源覆盖、legacy VM extension state。
- 随机种子和 replay log。

AI 生成内容一旦提交，必须作为确定性数据写入存档，而不是每次重新生成。

## 10. MVP 顺序

1. Core + Platform + Module + Property。
2. Actor/Component + EventBus + StateMachineRuntime。
3. Asset + Media + FilterGraph。
4. ScriptRuntimeHost + Astra Native Script。
5. AstraVN 最小 Dialogue/Choice/Character/Background。
6. Editor 基础和调试器。
7. AI 协作、Runtime Intent。
8. Legacy VN 模拟器和现代化插件。

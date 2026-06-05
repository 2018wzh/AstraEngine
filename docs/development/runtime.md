# Runtime 现状

## 1. 当前实现范围

当前代码已经包含 Phase 1 基础层和 Phase 2 headless Scene/Runtime 基座。

已实现：

- `Astra_Core`
- `Astra_Platform`
- `Astra_ModuleRuntime`
- `Astra_PropertySystem`
- `Astra_Scene`
- `Astra_Runtime`

仍未实现：

- `ScriptRuntimeHost`
- Asset / Media / FilterGraph
- AstraVN DSL 和 VN 预定义状态机
- Editor runtime debugger

## 2. Phase 2 运行时链路

Phase 2 的 headless 链路如下：

```text
RuntimeWorld
  -> ActorWorld
  -> RuntimeEventBus.enqueue()
  -> Director.tick()
  -> ControlPolicyRuntime.accept_or_queue()
  -> StateMachineRuntime.handle_event()
  -> WorldSnapshot / ReplayLog
```

对应样例可见 [AstraPhase2Smoke main.cpp](/E:/Documents/AstraEngine/Engine/Programs/AstraPhase2Smoke/Private/main.cpp)。

## 3. 当前模块职责

### Scene

提供：

- `ActorId`、`ActorTypeId`、`ComponentTypeId`
- `ActorHandle`
- `ComponentDescriptor`
- `ActorWorld`
- `ActorSnapshot`

`ActorId` 是稳定、可存档、可回放的公开身份。`ActorHandle` 只用于运行时访问，不写入存档。

### Runtime

提供：

- `RuntimeEventBus`
- `StateMachineRuntime`
- `BlackboardRuntime`
- `ControlPolicyRuntime`
- `Director`
- `WorldSnapshot`
- `ReplayRecorder` / `ReplayPlayer`
- `Transform2DSystemPack`

`RuntimeEventBus` 为 Phase 2 的主事件线。`Director` drain queued events，经 `ControlPolicyRuntime` 仲裁后推进 Actor-bound 状态机。

### Local ECS / Data-Oriented Pack

`Transform2DSystemPack` 使用 EnTT 做内部批处理存储。Pack 边界只暴露 `ActorId`、DTO 和 snapshot，不暴露 EnTT entity、native pointer 或 Actor C++ 指针。

## 4. Save / Load / Replay

`WorldSnapshot` 当前保存：

- snapshot version
- next actor id
- actors 和 component JSON data
- state machine state、blackboard、queued events
- blackboard runtime values
- control policy locks、priority table、queued events
- runtime event queue
- director tick/state
- next event sequence
- random seed

存档格式为 JSON，版本为 `astra.phase2.snapshot.v1`。存档不得保存 native pointer、`ActorHandle`、EnTT entity 或 renderer/audio native handle。

## 5. 当前不是运行时中心的东西

当前代码里不存在这些旧中心抽象：

- `VNRuntimeServices`
- `RuntimeCommand` 作为唯一主线
- `Bootstrap`
- `RuntimeProviderRegistry`

核心中心是 Actor、RuntimeEventBus、StateMachineRuntime、Blackboard、ControlPolicy、Director、Save/Replay。

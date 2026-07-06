# AstraEMU Legacy Runtime Framework

本页把 AstraEMU family contract 落到实现结构。目标不是做公共 VM，而是给 family core 一个统一的 runtime lifecycle：Manager 和 `RuntimeWorld` 只看到 session、step、effect、await、snapshot 和 report；opcode、syscall、tag、form、thread model 都留在 family 内部。

## Crate Shape

```text
astra-emu-family-api
  LegacyFamilyPluginDescriptor
  LegacyRuntimeProvider
  LegacyRuntimeSessionId
  LegacyStepInput / LegacyStepOutput
  LegacyEffect / LegacyWaitRequest
  LegacySnapshotEnvelope

astra-emu-manager
  RuntimeWorld bridge
  family enablement
  StateMachine action adapter
  local case report

astra-emu-family-*
  family private VM
  resource resolver
  presentation/audio mapper
  snapshot serializer
```

`astra-emu-family-api` 只放 DTO、trait 和 schema。它不能依赖 Manager UI、Editor、renderer backend、audio backend、旧 DLL、Luau/TJS VM 或 family crate。

## StateMachine Shape

StateMachine 只描述 legacy runtime 的粗粒度生命周期：

```text
Booting -> Active
Active -> Awaiting
Awaiting -> Active
Active -> Saving -> Active
Active -> Loading -> Active
Active -> Faulted
Active -> Shutdown
```

`legacy.step` 是主 action。adapter 根据 `LegacyStepOutput` 应用 effect、注册 AwaitToken、写 Blackboard status 和 trace。StateMachine 不展开旧 VM 的 opcode、syscall、tag、form、scene stack 或 script thread；这些细节只进入 bounded `StateMachineTrace` 和 family snapshot section。

## Session Lifecycle

1. Manager 根据 case profile 选择 family plugin。
2. Plugin Manager 校验 fingerprint、feature、permission、license 和 packaged eligibility。
3. Manager 调用 `probe` 生成 local metadata report，不执行商业脚本。
4. Manager 调用 `open`，provider 返回 `LegacyRuntimeSessionId`。
5. RuntimeWorld 创建 legacy Actor 和 StateMachine，并注册 action adapter。
6. 每个 fixed tick 调用 `step`，adapter 按顺序应用 `LegacyEffect`。
7. Save/load 调用 `save` 和 `restore`，只写公共 envelope 和 family opaque section。
8. 关闭 case 时调用 `shutdown`，并输出 final diagnostics。

Session 内可以持有 VM PC、stack、call stack、resource resolver、media state、text state、legacy save cursor 和 trace cursor。Session 外只能看到 stable id、hash、source span、resource ref、diagnostic 和 effect。

## Effect Ordering

`LegacyStepOutput.effects` 必须按旧引擎观察顺序排列。adapter 只做校验和转写，不重新排序 presentation/audio/text 行为。多个 producer 同 tick 输出时，Runtime 使用 `(tick_index, sequence, effect_id)` 生成 deterministic trace。

建议顺序：

1. wait resume 和 provider completion result。
2. VM control-flow trace。
3. text capture。
4. presentation command。
5. audio command。
6. Runtime event 和 Blackboard write。
7. AwaitToken 和 delayed event。
8. snapshot hint 和 diagnostics。

如果 family 引擎有更严格顺序，以 family trace 为准，但必须在 report 中声明 ordering policy。

## Await Bridge

family wait 先变成 `LegacyWaitRequest`，再由 adapter 创建 `AwaitToken`。Token 完成结果只在下一 fixed tick 进入 `LegacyStepInput.await_results`。平台 timer、音频回调、movie 结束、decode 完成和 async IO 都不能直接改 session state。

典型映射：

| Legacy wait | Runtime 映射 |
| --- | --- |
| frame/time wait | `AwaitToken` with deterministic timeout tick |
| click/key/choice | input await，消费 ordered input edge |
| transition/wipe/dissolve | presentation fence await |
| BGM/SE/voice/movie wait | audio/video media fence await |
| async resource/decode | provider completion await |
| family 私有等待 | `FamilyOpaque` + bounded trace |

## Snapshot And Replay

Snapshot 使用 `LegacySnapshotEnvelope`。公共 envelope 保存 family id、schema version、case fingerprint、runtime cursor、section hash 和 redaction；family section 保存 VM、presentation/audio、resolver 和 legacy save state。

Replay 不重新读取 wall-clock、OS callback 或 provider object address。它只消费 recorded input、await result、provider result 和 snapshot section。若 family section 缺 migrator、hash 不匹配或 redaction 不合格，restore 输出 blocking diagnostic。

## Family Usage

| Family | Session 私有内容 | Step 输出 |
| --- | --- | --- |
| Artemis | PFS resolver、`system.ini`、TagExecutor、LuaHost、layer/audio/save state | tag trace、legacy Lua diagnostic、presentation/audio、AwaitToken、snapshot section |
| KrKr | XP3 VirtualStorage、TJS VM、KAG conductor、PluginFacade | KAG/TJS trace、plugin capability diagnostic、media command、text capture、snapshot |
| BGI | archive index、BP/BCS VM、host dispatch、presentation/media state | VM dispatch trace、host call diagnostic、presentation/audio/movie effect、snapshot |
| SoftPAL | `ScriptRuntime`、extcall bridge、MemDat shadow、text/sprite/audio/save state | PAL wait、extcall trace、text/sprite/audio effect、memory snapshot |
| FVP | HCB parser、multi-context VM、syscall mapper、thread/presentation/audio state | context trace、syscall coverage、thread wait/text/dissolve effect、snapshot |
| Siglus | Scene package、Scene VM、form/element dispatch、Gameexe config、savepoint | scene/line trace、stage/message/audio/movie effect、selection/system wait、snapshot |
| Minori | PAZ reader、`.sc` decoder、VM、presentation/audio mapper | opcode trace、resource diagnostic、presentation/audio/text effect、snapshot |

## Release Gate

Release Gate checks:

- `LegacyRuntimeProvider` registered and selected by explicit project/case policy.
- `open` returns a session id and `shutdown` is called on normal exit.
- `step` effects are serializable and bounded.
- Await completion enters only through fixed tick input.
- Snapshot envelope roundtrips and carries family section hash.
- Report redaction omits payload, screenshots, audio samples, full script text, private absolute paths, key material and provider secrets.
- Replay hash matches recorded input and provider results.

The framework is a planned Stage 5 target. Current docs and research tools do not mean AstraEMU runtime code already exists.

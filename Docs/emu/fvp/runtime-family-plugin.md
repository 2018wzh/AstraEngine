# FVP Runtime Family Plugin Design

FVP family 以 engine-native plugin 接入 AstraEMU。Plugin 注册 `LegacyRuntimeProvider`，session 持有 `.hcb` VM、`.bin` archive set、syscall mapper、presentation/audio state、save state 和 diagnostics。Manager 负责窗口、平台输入、provider selection、overlay、报告和 release gate UI，并通过 `RuntimeWorld` 驱动 session。

## Session State

| 模块 | 职责 |
| --- | --- |
| `FvpRuntimeProvider` | 实现 lifecycle facade，管理 `LegacyRuntimeSessionId` |
| `FvpProbe` | 发现 `.hcb`、`.bin`、loose movie 和 cursor |
| `FvpArchiveSet` | pack metadata、VFS lookup 和 safe stream opening |
| `FvpScript` | HCB parser、NLS、syscall table 和 opcode decoder |
| `FvpVm` | context array、globals、return register、thread state 和 deterministic tick |
| `FvpSyscallMapper` | graph/text/audio/movie/input/save/thread syscall 实现 |
| `FvpPresentationState` | graph slots、prim tree、text slots、motion/dissolve state 和 cursor state |
| `FvpAudioState` | channel registry、bus/type volumes、fades 和 loaded media refs |
| `FvpSaveState` | VM snapshot 加 FVP graph/text/audio/history state |
| `FvpDiagnostics` | local structured trace ring、coverage counters 和 failure details |

这些类型都是 family-private。EngineCore 只看到 effects、Runtime events 和 opaque snapshot sections。

## Lifecycle

`probe` 只读 `.hcb`、`.bin` 和 loose media metadata。`open` 建立 archive set、HCB parser、VM contexts、syscall mapper 和 presentation/audio state。`step` 以 fixed tick 推进多 context VM，contexts 按稳定数字顺序运行，每个 context 执行到 exit 或 yield request。

外部完成事件不能立即应用。它先变成 ordered provider result 或 AwaitToken completion，再由下一次 `step` 消费。

Required stable ordering:

1. Input batch for tick.
2. Deferred text resume.
3. Safe-point load request.
4. Wait/sleep/dissolve timer advancement.
5. VM opcode/syscall loop.
6. Presentation/audio/text trace drain.
7. Snapshot capture.

## Step Output

FVP session 输出：

- `StateMachineTrace`：thread id、pc、opcode/syscall、arg count/type、yield reason。
- `PresentationCommand`：graph、prim、text slot、motion/dissolve state。
- `AudioCommand`：channel、volume、fade、currently loaded media ref。
- `TextCaptureEvent`：text slot hash、length、read state、speaker hash。
- `Diagnostic`：invalid opcode、missing syscall、stack underflow/overflow、snapshot safe-point failure。
- `LegacySnapshotEnvelope`：HCB identity、globals、contexts、thread manager、text、graph/motion、audio 和 VFS identity。

String bodies 和 media bytes 不进入 report。

## Save And Replay

FVP save state needs both VM-observable state and presentation state. Snapshot sections are opaque to Manager and versioned under `astraemu.fvp.*`. Runtime replay must not request network or external provider state.

## Release Gate Inputs

The FVP release gate accepts local structured case reports with HCB hash prefix、syscall count、archive entries、flow status 和 payload policy。The report is evidence for local compatibility, not a redistributable game sample.

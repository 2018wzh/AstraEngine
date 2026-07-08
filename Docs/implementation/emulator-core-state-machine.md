# EmulatorCore StateMachine Mapping

EmulatorCore 的目标是最大化复用 AstraEngine 自身能力：RuntimeWorld、StateMachine、AwaitToken、Asset/VFS、Media、Save/Replay、Plugin 和 Release Gate。旧引擎 VM 不进入 EngineCore 公共对象模型；它在 family provider 内部被映射成可审计的状态机执行模型。

## Layering

```text
AstraEmuRuntimeProvider
  -> LegacyRuntimeProvider family session
  -> family-private scheduler
  -> context state machines
  -> basic-block/action mapper
  -> LegacyEffect list
  -> DeterministicActionContext host adapter
```

`AstraEmuRuntimeProvider` 是 gameplay runtime provider。`LegacyRuntimeProvider` 是 family facade。family 内部可以把 opcode、tag、syscall、form、thread、fiber 或 coroutine 映射成私有 scheduler/context/basic-block/action 状态机，但这些状态不变成 EngineCore public StateMachine schema。

## VM Mapping

Family VM 至少拆成四层：

| 层 | 内容 | 进入公共 Runtime 的形态 |
| --- | --- | --- |
| scheduler | legacy thread/context/fiber 顺序、预算、wait queue、input edge | `StateMachineTrace`、diagnostic、snapshot cursor |
| context | PC、call stack、local/global variables、script resource id、current label | opaque family snapshot section hash |
| basic block | 连续可执行 opcode/tag/syscall，直到遇到 wait、branch、host call、fault 或预算结束 | ordered action trace |
| action bridge | 文本、选择、图像、音频、movie、timer、input、save/load、config、patch hook | `LegacyEffect`、`AwaitToken`、presentation/audio command、TextCaptureEvent |

Legacy VM 的 mutation 先进入 family-private candidate state。只有当 `step` 返回可序列化 `LegacyEffect` 后，host adapter 才在 fixed tick 边界提交到 `DeterministicActionContext`。Replay 读取录制的 await/provider result，不重新请求 platform provider 或 translation provider。

## Multi-context Scheduler

多线程或多 context 的旧 VM 使用多个 child state machine。调度顺序固定为：

```text
(priority, context_id, sequence)
```

`priority` 来自 family profile 或 VM 原始调度语义；`context_id` 是稳定 id；`sequence` 是 family session 内单调递增序号。一个 tick 内每个 context 在预算内推进，遇到 wait、yield、host call、fault 或 terminal 就停。Context 间通信必须变成有序 event、mailbox entry、blackboard mutation 或 family snapshot section，不能依赖宿主线程完成顺序。

## VFS Reuse

Family pack reader 必须实现 VFS mount provider。`.astrapkg` 保存 case profile、reader identity、package sections、release report 和 sanitized scenario；旧 pack 只作为 `legacy_pack` mount 被读取。报告只记录 alias、relative key、pack/entry、offset、size、hash、media kind、coverage 和 diagnostic，不记录本地 root 或 payload。

Patch、翻译资源、mod 和调试替换使用 `overlay` mount。没有 overlay allowlist 时，同 key 多命中必须 blocking。受保护、压缩未知或 hash 不可证明的 entry 不能退回线性扫描。

## Family Mapping

| Family | VM/脚本核心 | VFS mount | 状态机映射原则 |
| --- | --- | --- | --- |
| Artemis | PFS/PF6/PF8、`.iet` tag、legacy Lua block、ASB/table | `legacy_pack` for PFS，overlay for patch | tag executor 映射为 context state machine；tag filter 和 legacy call 只输出 deterministic effect |
| FVP | HCB VM、`.bin` pack、graph/text/sound/movie/thread syscall | `legacy_pack` for `.bin`，overlay for loose override | HCB basic block 映射为 private action sequence；syscall bridge 产生 presentation/audio/text/await effect |
| BGI | BURIKO ARC20/PackFile、DSC、BCS/BP VM、host dispatch | `legacy_pack` for archive | PC/stack/context 映射为 child state machine；host dispatch 必须有 syscall coverage 和 diagnostic |
| KrKr | XP3、KAG source、TJS bytecode、virtual storage | `legacy_pack` for XP3，overlay for patch | KAG label/context 和 TJS bytecode 分支分开建 context；unsupported bytecode 输出 reader-required diagnostic |
| Siglus | Scene.pck、Gameexe、`.ss`、G00/media | `legacy_pack` for Scene.pck/media | `.ss` instruction stream 映射 basic block；授权 material 缺失时阻断，不生成伪 effect |
| SoftPAL | PAC/DAT、script VM、extcall | `legacy_pack` for PAC/DAT | extcall 是 action bridge；未知 extcall 按 recoverable 或 blocking 分类 |
| Minori | PAZ、`.sc` script、演出命令 | `legacy_pack` for PAZ | `.sc` command cursor 映射 context；media command 走同一 presentation/audio bridge |

## FVP Detailed Example

FVP family session 打开后先通过 VFS 解析 `.bin` pack entry table，生成脱敏 entry map hash。`probe` 只记录 marker、entry count、HCB candidate、media kind histogram 和 diagnostic。`open` 创建主 HCB context，并按 HCB resource id 绑定 source map hash。

执行时：

1. scheduler 选择 `(priority, context_id, sequence)` 最小的 HCB context。
2. decoder 从当前 PC 读取 opcode，连续执行到 syscall、branch、wait、fault 或预算结束。
3. `GraphLoad`、`GraphMove`、`GraphFade` 变成 `PresentationCommand`，asset ref 使用 `fvp://` alias 和 VFS relative key。
4. `TextPrint` 变成 `TextCaptureEvent` 和 VN-like text presentation command，但不进入 AstraVN Core。
5. `SoundPlay`、`BgmPlay`、`VoicePlay` 变成 `AudioCommand`，completion 进入 `AwaitToken`。
6. `MoviePlay` 变成 video presentation command 和 media fence。
7. `ThreadStart` 创建 child context；`ThreadJoin` 等待 child terminal 或 timeout。
8. `Save` 只写 family snapshot envelope、Runtime save ref 和 redaction status。

FVP snapshot 保存 HCB PC、call stack、变量区、context list、pending wait、selected pack entry hashes 和 interpreter version。公共 report 不写 HCB bytecode、脚本文本、媒体 payload、本地 root 或可绕过访问控制的材料。

## Gate

EmulatorCore gate 至少验证：

- `AstraEmuRuntimeProvider` 已显式绑定，且 family `LegacyRuntimeProvider` 已通过 plugin descriptor gate。
- family scheduler/context trace 覆盖 boot、input、text、choice、media、save/load、shutdown。
- Await boundary、provider result、snapshot/replay hash 和 fault policy 可复现。
- VFS legacy pack mount、overlay mount 和 report redaction 通过。
- local case report 不含 payload-like 字段、本地绝对路径、完整脚本、截图、音频采样、provider secret 或访问控制绕过说明。

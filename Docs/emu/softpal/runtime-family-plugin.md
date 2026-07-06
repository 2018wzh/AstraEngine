# SoftPAL Runtime Family Plugin Design

SoftPAL family 通过 `LegacyRuntimeProvider` 接入 AstraEMU。Session 持有旧 SoftPAL 状态机、`ScriptRuntime`、resource catalog、extcall bridge、presentation/audio state、MemDat shadow 和 snapshot store。它不是 Astra Runtime 的 creator-facing 模型。

## Session Modules

| module | 职责 |
| --- | --- |
| `SoftPalRuntimeProvider` | 实现 `probe/open/step/save/restore/shutdown` |
| `CaseProbe` | 检查 game root、NLS、PAC、核心 DAT、script header、hash 和缺失项 |
| `ResourceCatalog` | 复现 ResourceManager 查找顺序，提供只读 asset reference |
| `ScriptImage` | 解析 `Sv20`、`POINT.DAT`、opcode metadata |
| `ScriptRuntime` | VM PC、stack、memory、wait、thread、save deterministic state |
| `ExtcallBridge` | 按 category/index 分派 text/sprite/audio/save/file/profile 等 handler |
| `PresentationBuilder` | 把 SoftPAL sprite/text state 转成 `PresentationCommand` |
| `AudioBridge` | 把 BGM/SE/voice/BGV 转成 `AudioCommand` |
| `SnapshotStore` | 保存和恢复 VM state，不保存 platform handle |
| `Diagnostics` | 输出 PC、opcode、extcall、resource、status、hash 和 concern |

保持这些模块足够直接。除非两个 family 真的共享同一行为，不新增跨 family 抽象。

## Lifecycle

```text
probe -> open session -> step ScriptRuntime
step -> wait -> step
step -> save/restore -> step
step -> shutdown
```

`probe` 识别 PAC/DAT、NLS、`SCRIPT.SRC`、`POINT.DAT`、`FILE.DAT`、`TEXT.DAT`、`MEM.DAT` 和 script check value。`open` 初始化 ResourceCatalog、ScriptRuntime、ExtcallBridge、PresentationBuilder 和 AudioBridge。`step` 固定 `pal_time_ms` 和 ordered input edge，按预算执行 VM instruction，直到 wait、unsupported opcode、unsupported extcall、fault 或 halt。

## Determinism

Session state 只受这些输入影响：

- loaded case metadata 和 resource bytes hash。
- fixed tick index 和 `pal_time_ms`。
- ordered input edge list。
- deterministic provider result，例如 media load success/failure code。
- previously restored snapshot。

SoftPAL wait 映射：

| SoftPAL wait | Framework 映射 |
| --- | --- |
| `Frame(n)` | frame `LegacyWaitRequest` |
| `Time(ms)` | deterministic timeout |
| `Click` | input await |
| `ClickOrTime(ms)` | input or timeout |
| `TextReveal(ms)` | presentation/text fence |

`pop_ext_args(count)` 的 native pop-order、`MemDatDirect` writable shadow 和 `MemDatIndirect` concern 都是 SoftPAL 私有语义，不能抽成公共 VM API。

## Step Output

SoftPAL session 输出 text/sprite/audio/history/save 等 ordered effects。Report 至少包含 family、NLS、script magic/check/entry、resources、extcall coverage 和 concerns。`extcallsKnown` 的实际值由实现后的 scanner 填充；文档不提前写成已达成。

## First Implementation Gate

1. Probe 本地样本，识别 SoftPAL 和核心资源。
2. Load `SCRIPT.SRC`、`POINT.DAT`、`FILE.DAT`、`TEXT.DAT`、`MEM.DAT`。
3. 执行到 title/menu 首个稳定 wait。
4. 输出 scene hash、sprite count、text state、BGM state、VM PC。
5. Save snapshot，reload 后 PC/memory/text/history 一致。

旧 VN 兼容不能成为 NativeVN、Editor 或 EngineCore 达标前置条件。SoftPAL route 自己通过 family release gate 即可。

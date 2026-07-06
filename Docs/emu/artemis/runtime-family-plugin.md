# Artemis Runtime Family Plugin Design

Artemis family 以 in-process plugin 接入 AstraEMU。Plugin 注册 `LegacyRuntimeProvider`，provider session 持有 PFS resolver、`system.ini`、script store、TagExecutor、LuaHost、presentation/audio state 和 snapshot cursor。Manager 创建 `RuntimeWorld`，StateMachine 只调用 legacy lifecycle action，不解析 Artemis tag、Lua 环境或 PFS patch 规则。

## Session Modules

| 模块 | 职责 |
| --- | --- |
| `ArtemisProbe` | 识别 data root、PF6/PF8 包、`system.ini`、boot script、movie header |
| `ArtemisRuntimeProvider` | 实现 `probe`、`open`、`step`、`save`、`restore`、`shutdown` |
| `ArtemisResolver` | loose/root/folder/patch lookup，PF6/PF8 streaming read |
| `SystemIniLoader` | 读取平台段、stage、charset、boot、save policy |
| `ScriptStore` | `.iet/.ast/.asb/.lua/.ipt/.tbl/.sli` 只读解析和 source map |
| `TagExecutor` | 内置 tag、macro、自定义 tag、Lua filter dispatch |
| `LuaHost` | Lua sandbox、engine object、deterministic host capability |
| `PresentationModel` | layer tree、message layer、transition fence |
| `AudioModel` | BGM、SE、voice、loop marker、fade |
| `SnapshotStore` | save/load、自描述 section、Lua serializable state |
| `TraceEmitter` | 本地结构化 trace、coverage、diagnostics |

EngineCore 不依赖 Artemis Lua、PFS、ASB、Windows movie backend 或具体 renderer/audio handle。

## Lifecycle

```text
probe -> open session -> step
step -> wait -> step
step -> save/restore -> step
step -> shutdown
```

`probe` 只输出 pack list、entry count、index hash prefix、`system.ini` boot metadata、script inventory、media inventory 和 diagnostics。`open` 建立 resolver、script store、LuaHost 和初始 presentation/audio state。`step` 执行 tag/Lua bridge，直到 wait、fault、halt、预算耗尽或 presentation boundary。

所有等待都通过 `LegacyWaitRequest` 进入 `AwaitToken`。异步 IO、图像加载、音频解码和视频 probe 可以并发，但完成结果只能在下一 fixed tick 回到 session。

## Step Output

Artemis session 输出：

- `TextCaptureEvent`：文本 hash、speaker id、voice ref、read flag。
- `PresentationCommand`：layer、message layer、transition、movie layer。
- `AudioCommand`：BGM、SE、voice、loop marker、fade。
- `StateMachineTrace`：script id、row index、tag name、Lua call hash、wait reason。
- `Diagnostic`：未知 tag、ASB branch、Lua sandbox blocked、media probe failure。
- `LegacySnapshotEnvelope` section：script stack、tag queue、Lua serializable state、media state 和 resolver fingerprint。

未知 tag、未知 ASB branch、不可序列化 legacy state 必须输出 `DONE_WITH_CONCERNS` 或 `BLOCKED`，不能写成已兼容。

## Public Report

Artemis family plugin 的 public contract 是 case profile + report，不是脚本编辑器。报告不包含 payload、完整脚本、截图、音频、视频帧或本机绝对路径。

## Implementation Order

1. Probe + PF6/PF8 resolver。
2. `system.ini` + boot `.iet` parser。
3. Tag trace executor，只输出 command。
4. Lua host allowlist + `calllua`/`setTagFilter`/`e:tag`。
5. Presentation/audio command 映射。
6. `.ast` command row parser。
7. ASB metadata probe，执行语义在 fixture 补齐后再启用。

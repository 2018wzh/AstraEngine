# AstraEMU BGI Runtime Family Plugin Design

BGI family 以 engine-native plugin 接入 AstraEMU。它在 `LegacyRuntimeProvider` session 内运行旧 VM、archive reader、resource decoder 和 presentation adapter；Manager 通过 `RuntimeWorld` 观察稳定 event、trace、media ref 和 snapshot section，不拥有旧引擎对象。

## Session Modules

```text
BgiFamilyPlugin
  BgiRuntimeProvider
  BgiCaseProfile
  BgiArchiveIndex
  BgiResourceStore
  BgiPayloadDecoder
  BgiScriptRegistry
  BgiVm
  BgiHostDispatch
  BgiPresentationModel
  BgiMediaRouter
  BgiSnapshotStore
  BgiTraceSink
```

| 模块 | 职责 |
| --- | --- |
| `BgiRuntimeProvider` | 实现 `probe/open/step/save/restore/shutdown` |
| `BgiCaseProfile` | 声明发行版本、archive roots、encoding、known quirks 和验收开关 |
| `BgiArchiveIndex` | 只读扫描 `.arc`，生成 entry metadata 和 resource lookup |
| `BgiResourceStore` | 按 resource id 懒读取 bytes，负责 hash、size、bounds check |
| `BgiPayloadDecoder` | 处理 DSC、CBG、raw image、audio box 和 movie magic |
| `BgiScriptRegistry` | 管理 BP/BCS program、source map 和 symbol |
| `BgiVm` | 执行 BP/BCS，维护 memory、stack、PC 和 await queue |
| `BgiHostDispatch` | 将 VM host call 转成 deterministic command |
| `BgiPresentationModel` | 维护 layer、object、surface、text window 和 transition state |
| `BgiMediaRouter` | 输出 audio/movie media block，不暴露 native handle |
| `BgiSnapshotStore` | 生成 save/replay snapshot section |
| `BgiTraceSink` | 输出 machine-readable trace 和 diagnostics |

## Lifecycle

```text
probe -> open session -> RunVmTick
RunVmTick -> AwaitInput -> RunVmTick
RunVmTick -> AwaitMedia -> RunVmTick
RunVmTick -> save/restore -> RunVmTick
RunVmTick -> shutdown
```

`probe` 检查 game root、archive magic 和 case profile。`open` 建立 archive index、script registry、VM 和 presentation/media state。`step` 按固定预算执行 VM；等待选择、点击、键盘、自动播放计时、animation、audio、movie 或 async resource 时，返回 `LegacyWaitRequest`。

## Step Output

BGI session 对 RuntimeWorld 的输出：

- `StateMachineTrace`：dispatch、resource load、decode diagnostic、VM stop reason。
- `TextCaptureEvent`：文本类别、source span、speaker id、message hash 和可显示短文本片段策略。
- `PresentationCommand`：layer/object/surface/text window 的增量变化。
- `AudioCommand`：audio/movie resource id、codec、timing、content-addressed media block id。
- `LegacySnapshotEnvelope`：VM、presentation、media、await queue 和 trace cursor。

输出中不得包含旧 VM 指针、renderer/audio native handle、Editor widget 或 plugin-owned object。

## 实现边界

- BGI family plugin 可使用 Tokio 或平台异步 IO，但 deterministic state 只在固定 tick 边界更新。
- Renderer2D 和 AudioGraph 只接收 decoded media block 或 presentation patch；不能接收 BGI native handle。
- Lua、Editor UI、MCP server、AI provider 和 legacy VM 细节都不能进入 EngineCore public contract。
- 旧 VN 兼容不是 NativeVN、Editor 或 EngineCore 达标前置条件；BGI family plugin 只作为 AstraEMU family adapter 推进。

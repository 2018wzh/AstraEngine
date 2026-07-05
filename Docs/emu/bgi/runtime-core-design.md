# AstraEMU BGI Runtime Core Design

BGI core 是 AstraEMU compat core 的一个 family 实现。它在独立进程内运行旧 VM、archive reader、resource decoder 和 presentation adapter；Manager 通过 IPC 观察状态，不拥有旧引擎对象。

## 模块

```text
BgiCoreProcess
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
| `BgiCaseProfile` | 声明发行版本、archive roots、encoding、known quirks 和验收开关。 |
| `BgiArchiveIndex` | 只读扫描 `.arc`，生成 entry metadata 和 resource lookup。 |
| `BgiResourceStore` | 按 resource id 懒读取 bytes，负责 hash、size、bounds check。 |
| `BgiPayloadDecoder` | 处理 DSC、CBG、raw image、audio box 和 movie magic。 |
| `BgiScriptRegistry` | 管理 BP/BCS program、source map 和 symbol。 |
| `BgiVm` | 执行 BP/BCS，维护 memory、stack、PC 和 await queue。 |
| `BgiHostDispatch` | 将 VM host call 转成 deterministic command。 |
| `BgiPresentationModel` | 维护 layer、object、surface、text window 和 transition state。 |
| `BgiMediaRouter` | 输出 audio/movie media block，不暴露 native handle。 |
| `BgiSnapshotStore` | 生成 save/replay snapshot section。 |
| `BgiTraceSink` | 输出 machine-readable trace 和 diagnostics。 |

## State machine

```text
Probe -> IndexArchives -> BootSystem -> RunVmTick
RunVmTick -> AwaitInput -> RunVmTick
RunVmTick -> AwaitMedia -> RunVmTick
RunVmTick -> Snapshot -> RunVmTick
RunVmTick -> Shutdown
```

- `Probe`：检查 game root、archive magic 和 case profile。
- `IndexArchives`：建立 archive index，不 decode 全量 payload。
- `BootSystem`：加载 `system.arc`、`ipl._bp`、`launcher._bp` 或 profile 指定入口。
- `RunVmTick`：固定 step budget 执行 VM。
- `AwaitInput`：等待选择、点击、键盘或自动播放计时。
- `AwaitMedia`：等待 animation、audio、movie 或 async resource 完成。
- `Snapshot`：写自描述 save section。
- `Shutdown`：释放 core 内部资源。

## IPC 输出

BGI core 对 Manager 的输出：

- `BgiTraceEvent`：dispatch、resource load、decode diagnostic、VM stop reason。
- `TextCaptureEvent`：文本类别、source span、speaker id、message hash 和可显示短文本片段策略。
- `BgiPresentationPatch`：layer/object/surface/text window 的增量变化。
- `BgiMediaBlock`：audio/movie resource id、codec、timing、shared memory block id。
- `BgiSnapshotReport`：snapshot section id、hash、version 和 replay cursor。

输出中不得包含旧 VM 指针、renderer/audio native handle、Editor widget 或 plugin-owned object。

## 数据结构草案

```text
BgiArchiveEntry {
  archive_path: Utf8PathBuf,
  name_raw: Vec<u8>,
  name: String,
  format: PackFile | BurikoArc20,
  table_index: u32,
  data_base: u64,
  relative_offset: u64,
  absolute_offset: u64,
  raw_size: u64,
  tail: Vec<u8>,
}
```

```text
BgiResourceId {
  case_id: String,
  archive_path: String,
  entry_name: String,
  decoded_kind: BgiPayloadKind,
}
```

```text
BgiSnapshot {
  version: u32,
  vm_state: BgiVmState,
  presentation_state: BgiPresentationState,
  media_state: BgiMediaState,
  await_queue: Vec<BgiAwaitToken>,
  trace_cursor: u64,
}
```

## 实现边界

- BGI core 可使用 Tokio 或平台异步 IO，但 deterministic state 只在固定 tick 边界更新。
- Renderer2D 和 AudioGraph 只接收 decoded media block 或 presentation patch；不能接收 BGI native handle。
- Lua、Editor UI、MCP server、AI provider 和 legacy VM 细节都不能进入 EngineCore public contract。
- 旧 VN 兼容不是 NativeVN、Editor 或 EngineCore 达标前置条件；BGI core 只作为 AstraEMU family adapter 推进。

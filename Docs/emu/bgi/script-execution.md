# BGI Script Execution

BGI script execution 由 archive reader、payload decoder、script parser、VM、host dispatch 和 presentation/media adapter 组成。AstraEMU BGI core 只输出稳定事件，不把旧 VM 对象暴露给 EngineCore。

## Load pipeline

1. `BgiArchiveIndex` 扫描 `.arc`，生成 entry metadata。
2. `BgiResourceStore::read(entry)` 读取 raw bytes。
3. `BgiPayloadDecoder` 如遇 `DSC FORMAT 1.00` 则 decode。
4. `BgiScriptProbe` 检测 BCS、BP 或 headerless scenario。
5. `BgiScriptParser` 生成 `BgiScriptProgram`、source map 和 diagnostics。
6. `BgiVm` 装载 program，并在固定 tick budget 内执行。
7. host dispatch 产出 `BgiTraceEvent`、`TextCaptureEvent`、`BgiMediaBlock` 和 presentation patch。

## VM state

BP 参考 VM 的重要常量：

| 名称 | 值 | 用途 |
| --- | ---: | --- |
| `INITIAL_MEMORY_SIZE` | `64 * 1024 * 1024` | 初始 VM memory。 |
| `LOCAL_MEMORY_BASE` | `0x0080_0000` | local memory base。 |
| `ADDRESS_MASK` | `0x01ff_ffff` | 地址掩码。 |
| `SYSTEM_PROGRAM_TABLE` | `273_280` | system program table 地址。 |
| `SYSTEM_PROGRAM_SLOTS` | `32` | program slot 数。 |
| `SYSTEM_PROGRAM_STRIDE` | `16` | program descriptor stride。 |
| `SYSTEM_PROGRAM_DESCRIPTOR_BASE` | `0x4000_0000` | descriptor base。 |

`BgiValue` 至少需要覆盖：

- `Int(i32)` 或 `u32` 兼容整数。
- `Str(BgiStringRef)`。
- `Ptr(u32)`。
- `Func { program_index, offset }`。
- `Program(BgiProgramId)`。
- `None`。

## BCS 与 BP 的协作

现代 BGI 常由 BP system program 加载 BCS scenario。`LoadProgram`/`LoadProgramEx` 读取 scenario entry 后，VM 需要记住 decoded BCS body range。`ScenarioCodePreprocess` 再把 BCS 中的 string reference shadow 到 VM memory，使老式 bytecode 读取字符串 offset 时能得到稳定地址。

参考行为中有两个关键 slot：

- `SCRIPT_CODE_BASE_SLOT = 0x0004_ca78`
- `SCRIPT_CURRENT_OFFSET_SLOT = 0x0004_cc70`

当旧 bytecode 对 BCS code range 后的 string pool 做 guarded read 时，若读越过 command end，参考 VM 会返回 BCS `ret` opcode `0x1B`，避免把 string pool 当作指令继续执行。AstraEMU 可以实现等价 guard，但必须在 trace 中记录 diagnostic。

## Tick loop

`BgiVm::run_tick` 输入为：

```text
BgiRunInput {
  tick: u64,
  max_steps: u32,
  pending_results: Vec<BgiAwaitResult>,
  input_events: Vec<BgiInputEvent>,
}
```

输出为：

```text
BgiRunOutput {
  stop_reason: Completed | MaxSteps | WaitingForInput | WaitingForAnimation | WaitingForMedia | Error,
  dispatches: Vec<BgiDispatchTrace>,
  text_events: Vec<TextCaptureEvent>,
  media_blocks: Vec<BgiMediaBlock>,
  presentation_patches: Vec<BgiPresentationPatch>,
  await_tokens: Vec<BgiAwaitToken>,
  diagnostics: Vec<BgiDiagnostic>,
}
```

`max_steps` 是防死循环保护，不是脚本语义。触发 `MaxSteps` 时 core 进入 paused diagnostic 状态，不能悄悄丢弃剩余脚本。

## Save/replay

BGI save/replay 必需状态：

- 当前 program id、PC、call stack、data stack。
- VM memory dirty ranges 或完整 snapshot。
- 已加载 archive/resource index version。
- BCS shadow table 和 source map id。
- presentation state：layer、object、surface、transition、text window。
- audio/movie state：slot、resource id、play position、loop flag、volume。
- await token queue 和已排序结果。
- RNG、timer 和 input sequence。

联网 provider 或平台 decoder 的结果必须在首次运行时固化为 deterministic event。回放只消费记录，不重新请求 provider 或依赖平台调度顺序。

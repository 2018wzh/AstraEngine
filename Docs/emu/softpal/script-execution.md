# SoftPAL Script Execution

SoftPAL VM 是 AstraEMU SoftPAL core 的权威状态机。它可以参考 `sena-rs::ScriptRuntime`，但在 AstraEMU 中必须通过 fixed tick、ordered event queue 和 serializable wait token 暴露行为。

## Boot state

Core load case 时建立：

- `pc = SCRIPT.SRC.entry_pc`
- `call_stack: Vec<u32>`
- `stack` 和 `argument_stack`
- `vars` 和多个 memory bank
- `user_mem`、`system_mem`、`temp_mem`，`sena-rs` 默认每个 `0x10000` 个 i32
- `mem_dat_words` writable shadow
- text、history、sprite、audio、button、select、save、thread 等 family 子状态
- `pal_time_ms`

`System.dat` 如果存在，`sena-rs` 会检查其中的 script check value 是否匹配 `SCRIPT.SRC` header。AstraEMU 也应把这个检查放进 Probe/Load diagnostics。

## Tick model

每个 `Step`：

1. 固定本 tick 的 `pal_time_ms` 和输入边缘。
2. 按预算执行 VM instruction。
3. 遇到 wait、unsupported opcode、unsupported extcall、fault 或 halt 时停止。
4. 把 text/sprite/audio/history/save 等副作用转成本 tick 的 ordered events。
5. 返回 `RuntimeStatus`、`FrameEvent`、diagnostics 和可选 wait token。

`sena-rs` 默认 runtime config 有 `instructions_per_frame`。AstraEMU core 可以保留预算参数，但 report 要记录预算值，否则 trace 不可复现。

## Wait

`sena-rs` 把 wait 抽象成：

| request | 语义 |
| --- | --- |
| `Frame(n)` | 等待 n 个 engine frame；`-1` 表示长期等待 |
| `Time(ms)` | 等待 PAL millisecond |
| `Click` | 等待任意 key/mouse push |
| `ClickOrTime(ms)` | click 或 timeout 二者先到 |
| `TextReveal(ms)` | 等文字 reveal 完成 |

AstraEMU 中这些 request 必须序列化成 `AwaitToken`，并且只在固定 tick 边界完成。不能直接把 OS timer、audio callback 或 async task 完成顺序写回 VM。

## Stack discipline

extcall 前脚本通常用 `push` 和 `pack_args` 把参数放入 argument stack。`sena-rs` 的 `pop_ext_args(count)` 按 native pop-order 返回：`pop[0]` 是最近 pack 的值。签名表中的 `display_order` 只是给 decompiler 和文档看，runtime handler 必须按 pop-order 读取。

清理规则：

- 已知签名按 `pop_count` 清理。
- unknown extcall 可以用 observed pop count 做 stack discipline fallback。
- fallback 只能返回安全默认值和 diagnostic，不能把 unknown 当已实现。

## Memory

`MemDatDirect` 写入 `mem_dat_words` shadow。save snapshot 要保存 shadow，load snapshot 要恢复 shadow。Core 不回写原始 `MEM.DAT` 文件。

`MemDatIndirect` 在 `sena-rs` 中仍是保守路径：读返回 0 并记录未实现，写忽略并记录。AstraEMU 如果沿用这个行为，必须在 release report 中标成 concern，不能把相关 route 算作 fully compatible。

## Save snapshot

`sena-rs` portable save 使用 `SENARSAV` magic 保存 PC、call stack、三类 memory、`mem_dat_words`、history records、text args/base/mode/visible。AstraEMU 不必复用这个二进制格式，但 SoftPAL snapshot 至少要覆盖同一批 deterministic state。

推荐 snapshot section：

```text
softpal.vm.pc
softpal.vm.call_stack
softpal.vm.user_mem
softpal.vm.system_mem
softpal.vm.temp_mem
softpal.vm.mem_dat_words
softpal.text.state
softpal.history.records
softpal.family.version
```

平台 audio handle、renderer texture id、window id、thread handle 不进入 snapshot。恢复后由 presentation/audio bridge 按状态重新建立。

## Diagnostics

每个 blocked point 输出：

```text
pc=0x00012340
opcode=extcall
category=0x0003
index=0x0002
name=sp_set
status=partial
reason=resource resolved but PGD3 base missing
```

release report 只保存 PC、编号、名字、状态和本地结构化原因；不保存商业文本、截图或资源 payload。

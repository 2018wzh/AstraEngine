# SoftPAL Script Format

SoftPAL 脚本由 `SCRIPT.SRC`、`POINT.DAT`、`FILE.DAT`、`TEXT.DAT` 和 `MEM.DAT` 共同解释。`SCRIPT.SRC` 保存 bytecode，`POINT.DAT` 保存 branch/call target，`FILE.DAT` 和 `TEXT.DAT` 把脚本里的 integer id 解析成资源名和文本，`MEM.DAT` 提供初始数据表。

## `Sv20` image

`SCRIPT.SRC` header 固定 12 byte：

```text
0x00  "Sv20"
0x04  check_value: u32 le
0x08  entry_pc: u32 le
0x0C  code_base
```

`sena-rs` 把 `SCRIPT_CODE_BASE` 定为 `12`。Koikake 样本的 `entry_pc` 是 `0x00000290`，说明 VM 不是从 `0x0C` 线性执行，而是跳到 header 指定入口。

## Instruction word

disassembler 每次读 little-endian u32：

- high 16 bit 等于 `1` 时，low 16 bit 是 primary opcode。
- high 16 bit 不等于 `1` 时，当作 data word。
- opcode 的参数也是连续 u32 word，但少数 opcode 有专门解码。

特殊参数规则：

| opcode | name | 参数规则 |
| ---: | --- | --- |
| `9` | `jmp_point` | 参数可先解成 operand，再尝试作为 point id |
| `10` | `jf_point` | `point_id` + condition operand |
| `11` | `gosub_point` | `point_id` |
| `20` | `lnot_slot` | raw slot |
| `23` | `extcall` | raw extcall id + destination slot |
| `29` | `neg_slot` | raw slot |

control-flow formatter 把 `end` 视为 halt，`ret` 视为 return，`wait` 类 opcode 视为 wait edge。分支和 call 的 target 都要通过 `POINT.DAT` 解析。

## Operand tag

operand 用最高 4 bit 表示 kind，low 16 bit 是 `lo`，中间 12 bit 是 `bank`：

| tag | kind | 解释 |
| ---: | --- | --- |
| `0x0` | `Immediate` | signed i32 immediate |
| `0x1` | `UserMemoryViaVar` | `user_mem[var[lo]]` |
| `0x2` | `SystemMemoryViaVar` | `system_mem[var[lo]]` |
| `0x3` | `StackSlot` | VM stack slot |
| `0x4` | `VariableSlot` | `var[lo]` |
| `0x5` | `TempMemoryViaVar` | temp memory with bank/base |
| `0x6` | `MemDatDirect` | `mem_dat_words[4 + bank + var[lo]]` |
| `0x7` | `MemDatIndirect` | indirect Mem.dat path，`sena-rs` 仍保守处理 |
| `0x8` | `ArgumentStack` | extcall argument stack |
| `0x9` | `ArgumentBase` | argument base marker |
| other | `LiteralSlot` | literal slot/sentinel |

`0x0FFF_FFFF` 常被当作空 string / no voice sentinel，`0x1000_0000` lane 用于 dynamic string id。不要在文档里把这些 sentinel 扩展成商业文本。

## Script string resolution

`ResourceStringFromFileDat` 先按 `FILE.DAT` slot 解析，再尝试 byte offset。`TextStringFromTextDat` 把值当 `TEXT.DAT` offset，优先读 `offset + 4` 后的 NUL string。解析失败时，诊断应保留 integer 和原因。

示例使用合成 id：

```text
push imm(102)
pack_args 1
extcall ext_0003_0002.sp_set dst_slot[0]

ResourceStringFromFileDat(102) -> FILE.DAT slot 102 -> "BK000D"  // synthetic fixture style
```

## Decompiler 输出边界

`pal-decompiler` 能把脚本转成 Lua-like source，并生成 extcall coverage report。AstraEngine 只保存 format 和 coverage 方法，不提交 decompiled commercial script。用于 release gate 的报告只能包含 PC、opcode/extcall 编号、状态、计数、hash 和本地结构化样例。

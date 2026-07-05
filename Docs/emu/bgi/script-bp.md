# BP System Program

BP 是 BGI system program 常用 bytecode，通常以 `._bp` entry name 出现在 `system.arc` 和 `sysprg.arc`。archive 内 raw payload 经常先被 `DSC FORMAT 1.00` 包裹，parser 必须在 DSC decode 后运行。

## Header

标准 BP header：

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x04` | `u32le` | `header_size`。观测样例多为 `0x10`。 |
| `0x04` | `0x04` | `u32le` | `instruction_size`。 |
| `0x08` | `0x08` | bytes | reserved。 |
| `header_size` | `instruction_size` | bytes | bytecode。 |
| `header_size + instruction_size` | variable | bytes | string pool 或 trailing data。 |

若 `header_size + instruction_size == len`，code range 可直接采用 header 字段。若不满足，core 可按 case profile 使用 headerless mode，并以最后一个 `ret` opcode `0x17` 后的位置作为 code end。

## Bytecode opcode

BP opcode 是 1 byte，operand 长度由 opcode 决定。核心指令：

| Opcode | 名称 | Operand | 说明 |
| ---: | --- | --- | --- |
| `0x00` | `push_byte` | `u8` | 压入 8-bit 整数。 |
| `0x01` | `push_word` | `u16le` | 压入 16-bit 整数。 |
| `0x02` | `push_dword` | `u32le` | 压入 32-bit 整数。 |
| `0x04` | `push_base_offset` | `u16le` | 压入 base offset。 |
| `0x05` | `push_string` | `i16le` | 字符串目标为 `current_offset + rel`。 |
| `0x06` | `push_offset` | `i16le` | 代码或数据 offset。 |
| `0x08` | `load` | `u8` | 从 VM memory 读。 |
| `0x09` | `move` | `u8` | 写 VM memory。 |
| `0x0A` | `move_arg` | `u8` | 写调用参数。 |
| `0x0C` | `copy_stack` | width, count | 复制栈段。 |
| `0x10` | `load_base` | none | 读 base。 |
| `0x11` | `store_base` | none | 写 base。 |
| `0x14` | `jmp` | target | 跳转。 |
| `0x15` | `jc` | kind | 条件跳转。 |
| `0x16` | `call` | target | 调用。 |
| `0x17` | `ret` | none | 返回。 |
| `0x80`/`0x81` | `sys` | call id | 系统 host call。 |
| `0x90`/`0x91`/`0x92` | `grp` | call id | 图形 host call。 |
| `0xA0` | `snd` | call id | 声音 host call。 |
| `0xB0`/`0xC0` | `usr` | call id | 用户/扩展 host call。 |

`BGITool\BGIBpScript` 确认 BP header 和 text pool 边界；`ethornell-rs` 给出 byte opcode 与 dispatch 表。`BGITool\BGIDisasm` 中的 dword opcode 表对应 BCS/older compiled script，不应误用于 BP bytecode。

## String pool

`push_string` 使用 signed relative offset。运行时要从 decoded BP bytes 中读取 NUL-terminated string，并保留原始 bytes 与 source offset。system program 字符串多为 resource name、program name 或 UI helper 参数，不能假设都是可显示文本。

## 观测样例

`E:\Games\樱之诗春之雪\system.arc:ipl._bp`：

- archive format：`PackFile`。
- raw DSC size：1,983 bytes。
- decoded BP length：2,608 bytes。
- `header_size`：16。
- `instruction_size`：2,592。
- code start：`0x10`。
- last `ret` 后的 code end：约 `0x82E`。

`E:\Games\サクラノ詩\sysprg.arc:scrmsg._bp`：

- archive format：`BURIKO ARC20`。
- raw DSC size：8,365 bytes。
- decoded BP length：19,904 bytes。
- `header_size`：16。
- `instruction_size`：19,888。
- code start：`0x10`。
- last `ret` 后的 code end：约 `0x4A31`。

`E:\Games\素晴らしき日々15th\system.arc:ipl._bp`：

- archive format：`BURIKO ARC20`。
- raw DSC size：2,283 bytes。
- decoded BP length：2,400 bytes。
- `header_size`：16。
- `instruction_size`：2,384。
- code start：`0x10`。
- last `ret` 后的 code end：约 `0x715`。

这些样例说明 `instruction_size` 可覆盖 string/trailing data；VM 执行时应以 parser 识别的 code range 为准，string resolver 可读取 code range 之后的数据。

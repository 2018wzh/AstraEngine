# BCS Script

`BurikoCompiledScriptVer1.00` 是现代 BGI scenario 的主格式。它是 dword opcode stream，字符串 offset 通常相对 `body_start`。

## Header

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x1C` | bytes | magic `BurikoCompiledScriptVer1.00\0`。 |
| `0x1C` | `0x04` | `u32le` | `header_size`。 |
| `0x20` | variable | bytes | namespace table、sub table 和其他 header bytes。 |
| `0x1C + header_size` | variable | bytes | command body，即 `body_start`。 |

header 内容按以下顺序读取：

1. `namespace_count: u32le`。
2. `namespace_count` 个 NUL-terminated CP932/Shift_JIS string。
3. `sub_count: u32le`。
4. `sub_count` 个 sub record，每项为 NUL-terminated sub name 加 `addr: u32le`。

实现必须把 `body_start = 0x1C + header_size` 固化为字符串和 command address 的基准。`BGITool` 和 `FuckGalEngine\BGI` 的工具都用这个基准查找 BCS text offset。

## Command stream

BCS command 以 `u32le opcode` 开始。部分 opcode 后跟一个或多个 `u32le` operand。核心 opcode：

| Opcode | 名称 | Operand | 说明 |
| ---: | --- | --- | --- |
| `0x000` | `push_dword` | value | 压入整数。 |
| `0x001` | `push_offset` | offset | 压入 body-relative offset。 |
| `0x002` | `push_base_offset` | offset | 压入 base-relative offset。 |
| `0x003` | `push_string` | offset | 字符串 offset，基准为 `body_start`。 |
| `0x07F` | `line` | string offset, line number | debug/source line。 |
| `0x018` | `jmp` | target | 跳转。 |
| `0x019` | `jc` | kind/target | 条件跳转，带额外 dword。 |
| `0x01A` | `call` | target | 调用。 |
| `0x01B` | `ret` | none | 返回。 |
| `0x03F` | `nargs` | count | 调用参数数量。 |
| `0x110` | `wait` | stack | 等待。 |
| `0x140` | `say` | stack | 对白或消息输出。 |

常用范围：

- `0x140..0x160`：message/text。
- `0x160..0x180`：choice/select。
- `0x180..0x200`：sound/movie。
- `0x200..0x400`：graph/presentation。

已观察到的具体 command：

| Opcode | 建议名称 | 说明 |
| ---: | --- | --- |
| `0x180` | `sound` | 声音相关 command。 |
| `0x1A0` | `sound_1a0` | 声音变体。 |
| `0x1B6` | `set_voice_seq` | voice sequence。 |
| `0x1BF` | `play_movie` | 影片播放。 |
| `0x240` | `bg240` | 背景变体。 |
| `0x260` | `bg` | 背景。 |
| `0x261` | `bg_transition` | 背景转场。 |
| `0x268` | `fade_to_black` | 黑场。 |
| `0x269` | `transition` | 通用转场。 |
| `0x280` | `sprite` | 立绘或对象显示。 |
| `0x288` | `sprite_hide` | 隐藏对象。 |
| `0x28A` | `sprite_hide_all` | 隐藏全部对象。 |

## Code end

BCS payload 后部可能包含 string pool。parser 不能把 string pool 当 command stream 执行。参考实现用最后一个 dword opcode `0x1B` 的结束位置作为 `code_end`；若找不到，则把文件尾作为保守上限并给出 diagnostic。

## 观测样例

`E:\Games\サクラノ詩\data01100.arc:00_op_01`：

- archive format：`BURIKO ARC20`。
- raw DSC size：34,372 bytes。
- decoded BCS length：114,699 bytes。
- `header_size`：36。
- `body_start`：`0x40`。
- namespace count：1，namespace 为 `Yuzu_2G`。
- sub count：1，`main` 地址为 `0`。
- `code_end`：`0x11750`。
- command count 约 8,936。
- 常见 opcode：`0x000` 约 3,870 次，`0x003` 约 1,910 次，`0x07F` 约 1,570 次，`0x140` 约 963 次，`0x280` 约 123 次，`0x110` 约 81 次。

`E:\Games\素晴らしき日々15th\data01101.arc:1-1_0710_dream`：

- raw DSC size：5,169 bytes。
- decoded BCS length：16,694 bytes。
- `header_size`：36。
- `body_start`：`0x40`。
- namespace count：0。
- sub count：1，`main` 地址为 `0`。
- `code_end`：`0x302C`。
- command count 约 1,547。

这些数字用于实现验收的 smoke test。测试断言应比较 header、count、offset、opcode histogram 和 hash，不写入完整台词文本。

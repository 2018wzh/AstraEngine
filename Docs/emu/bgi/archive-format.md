# BGI Archive Format

BGI core 的 archive reader 先建立只读索引，再按 resource request 懒加载 payload。索引阶段只读取 header、entry table 和有限 magic bytes，不展开完整商业资源。

## `PackFile`

`PackFile` 用于一个早期 BGI 合法样本的全部 12 个 `.arc`，也出现在一个现代 BGI 合法样本的少数 archive。

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x0C` | bytes | magic，前 8 bytes 为 `PackFile`，常见完整值为 `PackFile    `。 |
| `0x0C` | `0x04` | `u32le` | entry count。 |
| `0x10` | `count * 0x20` | entry table | 每项 32 bytes。 |
| `0x10 + count * 0x20` | variable | data block | entry offset 的基址。 |

Entry layout:

| Entry offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x10` | bytes | CP932/Shift_JIS name，NUL padded。 |
| `0x10` | `0x04` | `u32le` | relative offset。 |
| `0x14` | `0x04` | `u32le` | payload size。 |
| `0x18` | `0x08` | bytes | padding/reserved。 |

示例：`<bgi-packfile-case>/data01100.arc` 是 `PackFile`，entry count 为 5，data base 为 `0xB0`。entry `main` 的 relative offset 为 `0x0`，absolute offset 为 `0xB0`，raw size 为 726 bytes；entry `skp001` 的 relative offset 为 `0x2D6`，absolute offset 为 `0x386`，raw size 为 31,070 bytes。

## `BURIKO ARC20`

`BURIKO ARC20` 是现代 BGI/Ethornell 常见 archive。一个公开可复核的本地合法样本中，53 个 `.arc` 均为此格式。

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x0C` | bytes | magic `BURIKO ARC20`。 |
| `0x0C` | `0x04` | `u32le` | entry count。 |
| `0x10` | `count * 0x80` | entry table | 每项 128 bytes。 |
| `0x10 + count * 0x80` | variable | data block | entry offset 的基址。 |

Entry layout:

| Entry offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x60` | bytes | CP932/Shift_JIS name，NUL padded。 |
| `0x60` | `0x04` | `u32le` | relative offset。 |
| `0x64` | `0x04` | `u32le` | payload size。 |
| `0x68` | `0x18` | bytes | tail fields。不同发行可非零；core 先保留原值，不把它们当成 offset 或 size。 |

示例：`<bgi-modern-case>/data01100.arc` 是 `BURIKO ARC20`，entry count 为 5，data base 为 `0x290`。entry `00_op_01` 的 absolute offset 为 `0x290`，raw size 为 34,372 bytes。`<bgi-15th-case>/data01701.arc` 的 tail fields 存在非零值，但 payload 仍以 `DSC FORMAT 1.00` 开头，reader 不应拒绝。

## Payload decode order

archive reader 返回 `BgiRawPayload` 后，core 按固定顺序探测：

1. 若 payload 以 `DSC FORMAT 1.00` 开头，先执行 DSC decode。
2. 对 decoded bytes 重新探测 `BurikoCompiledScriptVer1.00`、BP header、`CompressedBG___`、raw image、audio box、movie magic。
3. 若 decoded bytes 仍未知，保留 raw magic、archive path、entry name、offset、size 和 hash 摘要，交给 case report。

`DSC FORMAT 1.00` layout:

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x10` | bytes | magic。 |
| `0x10` | `0x04` | `u32le` | key。 |
| `0x14` | `0x04` | `u32le` | decoded output size。 |
| `0x18` | `0x04` | `u32le` | decode count。 |
| `0x20` | `0x200` | bytes | 512 个 code depth byte。 |
| `0x220` | variable | bytes | Huffman/LZ payload。 |

DSC decode 规则：

- code depth byte 需按 key stream 还原，构造按 `(depth, code)` 排序的 Huffman code table。
- literal code `< 0x100` 直接写入输出。
- code `>= 0x100` 表示 back reference，offset 取 12-bit 值再加 2，copy count 取低 8-bit 再加 2。
- decoded size 必须等于 header 的 output size；不足或溢出都作为 decode error。

## Reader 验证

- `entry_count` 必须使 entry table 完整落在文件内。
- `data_base + relative_offset + size` 必须在 archive 文件长度内，使用 64-bit 中间值避免整数溢出。
- name 正规化时拒绝 `..`、root、drive prefix 和 path separator 穿越；原始 name bytes 仍保存在 trace 里。
- archive index 不写出 payload。需要提取测试 fixture 时，只允许自造小型文件或公开许可样本。

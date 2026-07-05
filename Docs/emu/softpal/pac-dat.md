# PAC / DAT Reference

本页是 SoftPAL binary data 的实现参考。数值来自 `sena-rs` parser 和本地 Koikake 样本的 metadata 检查。

## PAC

PAC 是按首 byte 分桶的资源包。每个条目 key 固定 32 byte，大小和 offset 为 little-endian u32。查找 key 时要先做 PAL 名称规范化：

- NLS encode，默认 Shift-JIS。
- `/` 改成 `\`。
- ASCII 小写转大写。
- 多字节字节段保持不变。
- encoded name 长度必须小于等于 32 byte。

本地样本中的 PAC header 前 16 byte 示例只作为结构证据：

```text
50 41 43 20 00 00 00 00 <entry_count_le> 00 00 00 00
```

不要依赖 header count 作为唯一真实值；bucket table 仍是遍历依据。

## `ARCHIVE.DAT`

`ARCHIVE.DAT` 是 path 列表，不是 PAC。parser 规则：

- 删除 CR、LF、space、tab 和 NUL。
- 按 `|` 分割。
- 用 NLS 解码每个 path。
- 把 `/` 统一成 `\`。

本地样本 path 包括 `movie`、`bgm`、`mask`、`em`、`ev`、`etc`、`bk`、`se`、`system`、`face`、`voice`、`st` 到 `st5` 和 `update*`。Probe 应把缺失的可选 update path 标成 absent，不要写成 fatal。

## `SCRIPT.SRC`

`SCRIPT.SRC` 不带 `$` marker。header：

| offset | size | meaning |
| ---: | ---: | --- |
| `0x00` | 4 | magic `Sv20` |
| `0x04` | 4 | check value |
| `0x08` | 4 | entry PC |
| `0x0C` | variable | code words |

Koikake 样本值：

```text
magic=Sv20
check=0x64CB7790
entry=0x00000290
size=0x00397A04
```

## `POINT.DAT`

`POINT.DAT` 在样本中是 `$` resource。`sena-rs` 解码后按 little-endian u32 table 使用。`PointTable::resolve_target_pc(point_id)` 的规则：

- `point_id == 0` 返回 `None`。
- `point_id > entries` 是越界错误。
- 目标 PC 为 `SCRIPT_CODE_BASE + offsets[entries - point_id]`。
- `SCRIPT_CODE_BASE` 固定为 `0x0C`。

这个 reverse-index 规则必须进入 decompiler、runtime 和 trace formatter 的共同 contract，否则 branch/gosub label 会错位。

## `FILE.DAT`

`FILE.DAT` 是 resource name slot table。解码后 layout：

| offset | size | meaning |
| ---: | ---: | --- |
| `0x00` | 16 | header，样本以 `$FILE_LIST__` 开头 |
| `0x10 + i * 0x20` | 32 | slot `i` 的 NUL-terminated string |

本地样本 `FILE.DAT` raw size 为 936,848 byte，对应 29,276 个 slot。extcall 中的 `ResourceStringFromFileDat` 通常先按 slot 解析；如果 slot 不可信，`sena-rs` 还会尝试把数值当 byte offset 读 NUL string。

## `TEXT.DAT`

`TEXT.DAT` 是 key + string record 列表。解码后 layout：

| offset | size | meaning |
| ---: | ---: | --- |
| `0x00` | 12 | magic `$TEXT_LIST__` |
| `0x0C` | 4 | entry count |
| `0x10` | variable | repeated `[key: u32][NUL string]` |

本地样本有 47,221 records。VM extcall 中的 `TextId` 通常是 `TEXT.DAT` byte offset，reader 会先尝试 `offset + 4` 后的 string，再尝试 offset 处的 string。

## `MEM.DAT`

`MEM.DAT` 解码后按 little-endian i32 words 使用。`sena-rs` 会把它载入 `mem_dat_words` writable shadow，脚本中的 `MemDatDirect` 写入只改 shadow，不回写商业资源文件。

operand 到 word index 的核心规则来自 `runtime.rs`：

```text
word_index = 4 + bank + vars[lo]
```

注释中的原始公式是 `mem_dat_ptr + 4 * (bank + vars[lo]) + 16`。因此 AstraEMU snapshot 要保存 `mem_dat_words`，并把 source DAT 当只读输入。

## `GRAPHIC.DAT`

`GRAPHIC.DAT` 是可选图形索引。`sena-rs` 当前 parser 处理 compact record：

| field | value |
| --- | --- |
| bucket count | 255 |
| bucket table offset | `0x10` |
| bucket table size | `0x7F8` |
| record size | `0x84` |

record 里有 32-byte key、replacement name、animation name 和 flags。`sena-rs` 保守地把 runtime expanded structure 里的 placement/scale lanes 置为中性值，避免把 compact file 的相邻 record 误读成位置元数据。AstraEMU 应沿用这个保守边界，等 loader expansion 有证据后再补。

# Artemis Archive Format

## 格式范围

Artemis 使用 `.pfs` 包文件保存资源。`pfs-rs` 覆盖 PF6 和 PF8：

| 格式 | magic | 读 | 写 | payload 加密 |
| --- | --- | --- | --- | --- |
| PF6 | `pf6` | 支持 | 不作为默认输出 | 无 |
| PF8 | `pf8` | 支持 | 支持 | 通过 index-derived SHA1 key 做 XOR |

官方 pack 文档把 PFS 作为普通文件系统看待，但明确限制一个包最大 2GB，并建议 Windows 全屏视频等非 MJA movie 留在包外。两个本地样本也把大视频放在 loose `movie/` 目录。

## Header 和 index

`pfs-rs` 的低层 parser 采用以下布局：

| Offset | 字段 | 类型 | 说明 |
| ---: | --- | --- | --- |
| `0x00` | magic | 3 bytes | `pf6` 或 `pf8` |
| `0x03` | `index_size` | u32 LE | 从 `0x07` 的 `index_count` 开始计入 |
| `0x07` | `index_count` | u32 LE | entry 数量 |
| `0x0B` | entries | repeated | name length、name、4 字节保留、offset、size |

单个 entry：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `name_length` | u32 LE | 文件名字节数 |
| `name` | bytes | 可能是 UTF-8，也可能是 Shift_JIS/CP932；archive 内常用 `\` 分隔 |
| reserved | 4 bytes | `00 00 00 00` |
| `offset` | u32 LE | payload 在包内的绝对 offset |
| `size` | u32 LE | payload 字节数 |

index 后还有 file-size offset table。`pfs-rs` writer 会记录每个 size 字段相对 `0x0F` 的 offset，写入 `index_count + 1` 个条目，最后一个为 8 字节零，再写入 `filesize_count_offset`。

## 编码和路径

文件名解码顺序应与 `pfs-rs` 保持一致：先尝试 UTF-8，再落到 Shift_JIS/CP932。AstraEMU 内部统一把 archive path 规范化为 `/`，但 diagnostics 应保留 raw path，方便定位与原包一致的问题。

路径安全要求：

- 拒绝空组件、`.`、`..`、NUL 和 Windows 保留字符。
- 写出 fixture 时必须确认目标仍在输出根目录内。
- Runtime resolver 只读 PFS 和 loose 文件，不把 archive entry 直接写回商业目录。

## PF8 加密

PF8 的 key 来自 index data：

```text
key = SHA1(archive[0x07 : 0x07 + index_size])
payload[i] = stored[i] XOR key[i % 20]
```

streaming 读写大文件时，key index 必须随 payload offset 继续递增。PF6 不生成 key，entry 都视为未加密。

`pfs-rs` 默认把 `mp4`、`flv` 视为未加密扩展。官方文档同时说明多数全屏视频不应放入 PFS；AstraEMU resolver 不能只靠扩展推断 movie 位置，必须先走 PFS/loose lookup，再交给 media probe 判断。

## Archive API

Artemis compat core 需要的最小 API：

| API | 输出 | 说明 |
| --- | --- | --- |
| `open_pack(path)` | format、entry count、index hash | 只读 header/index |
| `list_entries(pack)` | normalized path、raw path、offset、size、encrypted | 不读 payload |
| `read_entry(path)` | streaming reader | 按 PF6/PF8 解密规则输出 payload bytes |
| `find(path)` | resolver hit | loose、root pack、folder pack、patch chain 合并后的命中信息 |
| `diagnose_pack(path)` | machine-readable report | format、损坏 index、越界 entry、重复 path 和加密状态 |

## 样本观察

| 样本 | 根包 | 格式 | entry 数 | 主要扩展 |
| --- | --- | --- | ---: | --- |
| サクラノ詩10th | `sakuranouta10th.pfs` | PF8 | 1,823 | `.png`、`.ast`、`.jpg`、`.ogg`、`.lua`、`.ipt`、`.iet`、`.asb` |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs` | PF8 | 10,919 | `.ogg`、`.png`、`.sli`、`.ast`、`.lua`、`.asb`、`.iet` |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs.721.bak` | PF6 | 117 | `.ast`、`.lua`、`.asb`、`.iet`、`.tbl` |

PF6 backup 包证明 probe 不能把 Artemis PFS 简化成 PF8-only。是否参与 runtime lookup 由 patch-chain 文件名规则决定，不由内部 magic 单独决定。

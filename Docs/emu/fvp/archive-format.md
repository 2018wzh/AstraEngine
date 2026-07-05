# FVP Archive Format

FVP 资源主包是 root 目录下的 `<folder>.bin`。rfvp 的 `VfsFile::parse_reader` 和样本观察一致，结构很小：

```text
u32 file_count
u32 filename_table_size
Entry[file_count]
FilenameTable[filename_table_size]
Payload...

Entry:
  u32 name_offset
  u32 data_offset
  u32 data_size
```

`name_offset` 指向 filename table 内的 NUL-terminated string。`data_offset` 是相对整个 `.bin` 文件开头的绝对偏移，不是相对 payload 区。字符串按 case NLS 解码，常见值是 Shift_JIS；汉化样本可能需要 GBK/GB18030 或补丁层覆盖。

## VFS 路径

VFS key 使用 `folder/name`。加载 `graph_bg.bin` 后，脚本里的 `graph_bg/BG001_000` 会解析到 `graph_bg` pack 的 `BG001_000` entry。desktop host 还允许 loose file 覆盖：如果 `<game-root>/<folder>/<name>` 存在，优先读 loose file，再读 pack entry。

路径规范化规则：

- 反斜杠转成 `/`。
- 去掉开头的 `./` 和 `/`。
- pack folder 按小写索引。
- archive reader 不做目录遍历；movie cache 也必须拒绝 absolute path 和 `..`。

## 样本统计

| Pack | Entries | Filename table | Metadata bytes | Payload magic |
| --- | ---: | ---: | ---: | --- |
| `bgm.bin` | 70 | 280 | 1,128 | `OggS` |
| `voice.bin` | 14,498 | 130,482 | 304,466 | `OggS` |
| `se.bin` | 304 | 1,223 | 4,879 | `RIFF` |
| `se_env.bin` | 79 | 316 | 1,272 | `RIFF` |
| `se_sys.bin` | 13 | 52 | 216 | `RIFF` |
| `graph.bin` | 1,146 | 12,330 | 26,090 | `hzc1` |
| `graph_bg.bin` | 375 | 3,929 | 8,437 | `hzc1` |
| `graph_bs.bin` | 594 | 13,683 | 20,819 | `hzc1` |
| `graph_sd.bin` | 57 | 513 | 1,205 | `hzc1` |
| `graph_vis.bin` | 579 | 6,849 | 13,805 | `hzc1` |
| `graph_vish.bin` | 380 | 4,339 | 8,907 | `hzc1` |
| `patch.bin` | 71 | 813 | 1,673 | `hzc1` |

Concrete lookup examples from the sample:

| VFS path | Pack entry | Offset | Size | Observation |
| --- | --- | ---: | ---: | --- |
| `bgm/001` | `001` in `bgm.bin` | 1,128 | 2,689,312 | Ogg Vorbis |
| `voice/01000010` | `01000010` in `voice.bin` | 304,466 | 17,889 | Ogg Vorbis |
| `se/001` | `001` in `se.bin` | 4,879 | 427,456 | RIFF/WAVE-like payload |
| `graph/BG001_000` | `BG001_000` in `graph_bg.bin` | 8,437 | 2,852,204 | `hzc1` texture payload |
| `movie/01.wmv` | loose file | n/a | 111,307,131 | ASF/WMV, not packed |

## AstraEMU reader contract

The FVP archive reader should be a family-private service behind the core VFS. It exposes:

- `probe_pack(path) -> PackMetadata` for report and diagnostics.
- `open_entry(folder, name) -> Read + Seek` for media decoders.
- `read_small_entry(folder, name, max_bytes)` for scripts, fonts, cursor metadata and test fixtures.
- `hash_entry(folder, name)` for case reports.

It must not expose host filesystem paths to Manager reports. Reports store pack name, entry name, offset, size, hash prefix and media kind only.

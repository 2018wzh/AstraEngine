# SoftPAL Source Inventory

本页只记录可验证的代码入口和本地样本元数据。商业资源的完整内容、解包文件、文本导出和截图不进入 AstraEngine。

## `sena-rs` 代码入口

| 路径 | 作用 |
| --- | --- |
| `crates/pal-asset` | PAC reader、`ARCHIVE.DAT` parser、NLS 编码、`$` 资源解码、ResourceManager |
| `crates/pal-script` | `Sv20` header、operand、disassembler、primary opcode 表、extcall signature 表 |
| `crates/pal-vm` | SoftPAL VM、runtime state、sprite/text/audio/save/wait handler、launcher |
| `crates/pal-decompiler` | `SCRIPT.SRC` + `POINT.DAT` + `FILE.DAT` + `TEXT.DAT` + `MEM.DAT` 的本地分析工具 |
| `crates/pal-pac-unpacker` | PAC metadata listing 和本地 unpack 工具；AstraEngine 文档只使用 list-only 信息 |
| `crates/pal-assets` | ResourceManager 验证 CLI，支持 paths、loaded PAC list 和单资源 preview |
| `crates/na_wmv_player` | ASF/WMV2/WMA decoder 试验实现，供 movie path 参考 |
| `platform/*` | `sena-rs` 的桌面、移动、Web launcher；AstraEMU 不继承这些 UI 壳 |

`sena-rs` README 明确说明它是 SoftPAL engine 的跨平台 Rust 实现，目标样本是 Koikake。运行入口是 `pal-vm` 的 `sena` binary，默认 `Nls::ShiftJis`，也支持 `gbk` 和 `utf-8`。

## 本地 Koikake 样本矩阵

本机样本检查到 20 个顶层 PAC。表中只列 archive 名、entry 数、文件大小和主扩展名，不包含 payload。

| PAC | entries | size bytes | 主要内容 |
| --- | ---: | ---: | --- |
| `data.pac` | 323 | 13,801,286 | `SCRIPT.SRC`、DAT、ANI、MIX、CSV、INI |
| `bgm.pac` | 30 | 71,875,309 | OGG BGM |
| `se.pac` | 449 | 22,366,271 | OGG SE |
| `voice.pac` | 11,898 | 652,909,082 | OGG voice |
| `bk.pac` | 107 | 374,546,425 | PGD background |
| `ev.pac` | 192 | 160,188,389 | PGD event CG |
| `face.pac` | 626 | 163,667,924 | PGD face |
| `st*.pac` | 2,183 total | 4,973,723,126 total | PGD standing sprites |
| `etc*.pac` | 251 total | 1,196,886,196 total | PGD/TGA shared and localized UI |
| `system*.pac` | 494 total | 47,812,190 total | PGD/OGG/TGA system UI |
| `mask.pac` | 20 | 124,419,940 | BMP masks |
| `em.pac` | 5 | 10,897,666 | MPG and PGD |

Loose directory metadata:

| directory | files | 观察 |
| --- | ---: | --- |
| `movie` | 1 | `opening.mpg` |
| `movie_cn` | 1 | localized `opening.mpg` |
| `movie_tc` | 1 | localized `opening.mpg` |
| `fonts` | 3 | bundled TTF |
| `dll` | 10 | legacy Windows DLLs; AstraEMU core does not load them |

`data.pac` 中的核心资源观察：

| resource | raw size | encrypted marker | 解析事实 |
| --- | ---: | --- | --- |
| `ARCHIVE.DAT` | 112 | no | path list uses `|` separators |
| `SCRIPT.SRC` | 3,766,020 | no | magic `Sv20`, check `0x64CB7790`, entry `0x00000290` |
| `POINT.DAT` | 17,240 | yes | 4-byte aligned table after `$` resource transform |
| `FILE.DAT` | 936,848 | yes | 0x10 header, 29,276 slots of 0x20 bytes |
| `TEXT.DAT` | 3,565,676 | yes | magic `$TEXT_LIST__`, 47,221 records |
| `MEM.DAT` | 7,604 | yes | parsed as little-endian i32 words |
| `GRAPHIC.DAT` | 13,684 | yes | compact graphic index, optional at runtime |

## 资源路径观察

`ARCHIVE.DAT` 在本地样本中列出的 path 为：

```text
movie, bgm, mask, em, ev, ev2, etc, bk, se, system, face, voice,
st, st2, st3, st4, st5, update, update2, update3, update4, update5
```

其中样本根目录没有 loose `ARCHIVE.DAT`；ResourceManager 先把 `data` 加进查找路径，再从 `data.pac` 打开 `archive.dat`，随后追加这些 path。部分 path 是 patch/update 预留或安装可选目录；Probe 阶段只报告存在性和 hash，不把缺失的可选 path 视为 fatal。

## 代码事实和 AstraEMU 取舍

`sena-rs` 是可运行参考，不是 AstraEMU 的 API 边界。AstraEMU 可以复用格式知识、测试思路和局部算法，但不能把 `sena-rs` launcher、平台 UI、software renderer、Kira audio handle 或 app bundle 结构带进 EngineCore。SoftPAL provider session 的输出必须先落到 `LegacyRuntimeProvider` contract，再由 Manager 转成 Astra Runtime / Media / Release Gate 可观测事件。

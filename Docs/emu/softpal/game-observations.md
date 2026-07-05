# Koikake Game Observations

本页记录一次本地 Koikake 安装目录的本地结构化观察。目录绝对路径不写入文档；只保留资源矩阵、文件大小、entry 数、hash 前缀和格式事实。

## Top-level layout

| item | observation |
| --- | --- |
| executable | `Game.exe` present |
| PAC count | 20 |
| loose movie dirs | `movie`、`movie_cn`、`movie_tc` |
| loose font dir | 3 TTF |
| legacy DLL dir | 10 DLL，AstraEMU 不加载 |
| Steam marker | `steam_appid.txt` present |

## PAC inventory

| PAC | entries | size bytes | dominant extensions |
| --- | ---: | ---: | --- |
| `bgm.pac` | 30 | 71,875,309 | `.OGG` |
| `bk.pac` | 107 | 374,546,425 | `.PGD` |
| `data.pac` | 323 | 13,801,286 | `.ANI`、`.MIX`、`.DAT`、`.CSV` |
| `em.pac` | 5 | 10,897,666 | `.MPG`、`.PGD` |
| `etc.pac` | 179 | 1,111,705,423 | `.PGD`、`.TGA` |
| `etc_cn.pac` | 36 | 42,981,816 | `.PGD` |
| `etc_tc.pac` | 36 | 43,313,257 | `.PGD` |
| `ev.pac` | 192 | 160,188,389 | `.PGD` |
| `face.pac` | 626 | 163,667,924 | `.PGD` |
| `mask.pac` | 20 | 124,419,940 | `.BMP` |
| `se.pac` | 449 | 22,366,271 | `.OGG` |
| `st.pac` | 384 | 1,106,745,606 | `.PGD` |
| `st2.pac` | 421 | 864,566,213 | `.PGD` |
| `st3.pac` | 384 | 886,809,116 | `.PGD` |
| `st4.pac` | 374 | 1,051,076,841 | `.PGD` |
| `st5.pac` | 620 | 1,150,755,080 | `.PGD` |
| `system.pac` | 209 | 11,862,209 | `.PGD`、`.OGG`、`.TGA` |
| `system_cn.pac` | 143 | 23,875,991 | `.PGD` |
| `system_tc.pac` | 142 | 24,159,490 | `.PGD` |
| `voice.pac` | 11,898 | 652,909,082 | `.OGG` |

## Core records in `data.pac`

| resource | bucket | record | raw size | data offset | hash prefix |
| --- | ---: | ---: | ---: | ---: | --- |
| `ARCHIVE.DAT` | 65 | 257 | 112 | `0x65F3` | `6ddba5f0ef45089a` |
| `SCRIPT.SRC` | 83 | 272 | 3,766,020 | `0x627485` | `b0c4936db9c331c8` |
| `POINT.DAT` | 80 | 270 | 17,240 | `0x622FB2` | `01c2748bf0b2d9a7` |
| `FILE.DAT` | 70 | 265 | 936,848 | `0x538361` | `6e49063d8a858cc0` |
| `TEXT.DAT` | 84 | 276 | 3,565,676 | `0x9C146F` | `9d0b2bcf561f89c8` |
| `MEM.DAT` | 77 | 269 | 7,604 | `0x6211FE` | `0d21451b3ed640ad` |
| `GRAPHIC.DAT` | 71 | 267 | 13,684 | `0x61D6AD` | `80e57fe9871e4abd` |

`SCRIPT.SRC` header:

```text
magic=Sv20
check=0x64CB7790
entry=0x00000290
```

`TEXT.DAT` 解码后记录数为 47,221。`FILE.DAT` 解码后有 29,276 个 0x20-byte slot。`POINT.DAT` 和 `MEM.DAT` 是 `$` resource，解码后按 u32/i32 word 读取。

## Archive paths

`ARCHIVE.DAT` path list：

```text
movie | bgm | mask | em | ev | ev2 | etc | bk | se | system | face | voice |
st | st2 | st3 | st4 | st5 | update | update2 | update3 | update4 | update5
```

样本里有 localized loose movie dirs 和 localized system/etc PAC，但 archive path 仍以基础 path 为主。AstraEMU Probe 应同时扫描实际顶层 PAC 和 `ARCHIVE.DAT` path，报告两者差异。

## Media observations

- BGM、SE、voice 均为 OGG。
- CG、背景、立绘、face 主要为 PGD。
- mask 是 BMP。
- opening movie 是 MPG，存在三套 loose localization 目录。
- `system.pac` 同时有 PGD、OGG 和 TGA。

## Risk notes

`ev2`、`update*` 出现在 `ARCHIVE.DAT` 但本次顶层列表没有对应 PAC。它们可能是 patch path 或可选安装内容。Probe 不应直接判定样本损坏，除非实际 route 需要这些资源并且 resolver 命中失败。

`dll` 目录包含 legacy Windows DLL。AstraEMU SoftPAL core 不加载它们；相关功能通过 Rust/native provider 或 no-op diagnostic 实现。

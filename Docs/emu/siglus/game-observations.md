# Siglus Game Observations

本页记录两个本地样本的本地结构化观测。观测只包括文件计数、header、索引和资源名片段，不包含脚本文本、画面、音频、视频或 key。

## anemoi 体験版

Root：`<siglus-anemoi-case>`。实际游戏数据位于 `StartData/GameData`。

扩展名计数：

| 扩展名 | 数量 |
| --- | ---: |
| `.g00` | 984 |
| `.ogg` | 169 |
| `.omv` | 27 |
| `.ovk` | 21 |
| `.owp` | 32 |
| `.pck` | 1 |
| `.dat` | 1 |
| `.gan` | 5 |
| `.wmv` | 1 |
| `.sav` | 5 |

`Scene.pck`：

| 字段 | 值 |
| --- | ---: |
| size | 2,579,930 |
| has_signature | false |
| header_size | 92 |
| inc_prop_cnt | 0 |
| inc_cmd_cnt | 614 |
| scn_name_cnt | 104 |
| scn_data_cnt | 104 |
| scn_data_list_ofs | 44,412 |
| scn_data_exe_angou_mod | 1 |
| original_source_header_size | 614 |

`Gameexe.dat`：

| 字段 | 值 |
| --- | --- |
| size | 15,059 |
| first 8 bytes | `00 00 00 00 01 00 00 00` |
| interpreted header | `version=0, exe_angou_mode=1` |

媒体 header 例子：

| 文件 | 观测 |
| --- | --- |
| `g00/__face_mask.g00` | G00 type 2，1920x1080，size 200,641 |
| `mov/ef_aurora_slow.omv` | header size 168，version 257，RGBA，1920x1080，33333 us/frame，`OggS` offset 32908 |
| `koe/z0001.ovk` | 14 entries，entry 0 `size=51021, offset=228, no=59, sample_count=157298` |
| `bgm/M01A.owp` | size 5,419,529，首 4 bytes 不是 `OggS` |

## Rewrite_PLUS

Root：`<siglus-rewrite-case>`。

扩展名计数：

| 扩展名 | 数量 |
| --- | ---: |
| `.g00` | 4144 |
| `.g01` | 656 |
| `.ogg` | 57 |
| `.omv` | 189 |
| `.ovk` | 157 |
| `.nwa` | 395 |
| `.mpg` | 2 |
| `.wmv` | 3 |
| `.pck` | 1 |
| `.dat` | 1 |
| `.chs` | 2 |

`Scene.pck`：

| 字段 | 值 |
| --- | ---: |
| size | 17,338,693 |
| has_signature | false |
| header_size | 92 |
| inc_prop_cnt | 49 |
| inc_cmd_cnt | 148 |
| scn_name_cnt | 160 |
| scn_data_cnt | 160 |
| scn_data_list_ofs | 16,452 |
| scn_data_exe_angou_mod | 1 |
| original_source_header_size | 870 |

前几个 scene name：

```text
0  __va_effect_ss_cmd_particle
1  ed_akane
2  ed_chihaya
3  ed_common
4  ed_kotori
5  ed_lucia
6  ed_shizuru
7  seen01000
```

`Gameexe.*`：

| 文件 | size | first 8 bytes | interpreted header |
| --- | ---: | --- | --- |
| `Gameexe.dat` | 19,193 | `00 00 00 00 01 00 00 00` | `version=0, exe_angou_mode=1` |
| `Gameexe.chs` | 32,012 | `00 00 00 00 01 00 00 00` | `version=0, exe_angou_mode=1` |

媒体 header 例子：

| 文件 | 观测 |
| --- | --- |
| `g00/BG001.g00` | G00 type 0，1280x720，size 2,012,485 |
| `mov/ef_ak_da_aura00.omv` | header size 168，version 257，RGBA，500x460，33333 us/frame，`OggS` offset 5940 |
| `koe/z1001.ovk` | 8 entries，entry 0 `size=59236, offset=132, no=11, sample_count=94100` |
| `koe/2036/Z203600522.nwa` | stereo，16-bit，44100 Hz，`pack_mod=-1`，`sample_cnt=155700` |
| `mov/op01.mpg` | MPEG sequence header offset 2078，1280x720，`frame_rate_code=1` |
| `bgm/BGM001.ogg` | 首 4 bytes 为 `OggS` |

## 实现影响

1. Resolver 必须支持 root 布局和 `StartData/GameData` 布局。
2. `Gameexe.chs` 这类变体可能覆盖或补充 `Gameexe.dat`，不能只硬编码一个文件名。
3. G00 type 0 和 type 2 都是首阶段必需；只支持裸 PNG/JPEG 不够。
4. 视频至少要区分 OMV、MPEG 和 WMV provider。
5. 语音既可能来自 OVK pack，也可能来自 NWA 文件树。

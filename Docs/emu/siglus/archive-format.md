# Siglus Archive And Resource Format

Siglus 的资源形态更接近“目录 + 若干专用容器”，不是单一大包。AstraEMU core 应先按目录 family 探测，再按文件名和 header 做格式分派。

## 目录布局

两个样本都保留明文目录：

| 样本 | 典型路径 | 说明 |
| --- | --- | --- |
| anemoi 体験版 | `StartData/GameData/Scene.pck` | 体验版把游戏数据放在启动器目录下 |
| anemoi 体験版 | `g00/`, `bgm/`, `koe/`, `mov/`, `wav/`, `savedata/` | 图像、BGM、语音包、视频、SE、存档 |
| Rewrite_PLUS | `Scene.pck` | 根目录直接放 scene package |
| Rewrite_PLUS | `g00/`, `bgm/`, `koe/`, `mov/`, `mov_chs/`, `wav/`, `dat/` | 同时存在日文与汉化资源 |

Core 的 resolver 不应假设 `Scene.pck` 一定位于 root。最低候选顺序：

1. `project_dir/Scene.pck`
2. `project_dir/scene.pck`
3. `project_dir/Data/Scene.pck`
4. `project_dir/data/Scene.pck`
5. 启动器布局下的 `StartData/GameData/Scene.pck`

## Scene.pck

`Scene.pck` 是脚本和场景元数据容器。`siglus_rs` 的 `PackScnHeader` 和旧 `SCENEHEADER` 都显示 header 由 little-endian `i32` 字段组成。较新/较旧 build 可带或不带 ASCII signature `pack_scn`；两个本地样本都没有 signature，首字段直接是 `header_size = 92`。

核心表：

| 字段族 | 作用 |
| --- | --- |
| `inc_prop_*` | package 级 include property 表，跨 scene 共用 |
| `inc_cmd_*` | package 级 include command 表，跨 scene 共用 |
| `scn_name_*` | UTF-16LE scene name index + string list |
| `scn_data_index_list` | 每个 scene chunk 的 offset/size |
| `scn_data_list` | scene chunk payload 起点 |
| `scn_data_exe_angou_mod` | scene chunk 是否使用 exe 侧保护材料 |
| `original_source_header_size` | 非零时，chunk 按原始编译产物路径处理 |

样本 header：

| 样本 | size | inc_prop_cnt | inc_cmd_cnt | scn_name_cnt | scn_data_cnt | scn_data_list_ofs | scn_data_exe_angou_mod |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| anemoi 体験版 | 2,579,930 | 0 | 614 | 104 | 104 | 44,412 | 1 |
| Rewrite_PLUS | 17,338,693 | 49 | 148 | 160 | 160 | 16,452 | 1 |

Rewrite_PLUS 的前几个 scene name 是 ASCII，可安全作为索引例子：`__va_effect_ss_cmd_particle`、`ed_akane`、`ed_chihaya`、`ed_common`、`ed_kotori`、`ed_lucia`、`ed_shizuru`、`seen01000`。

## Scene chunk 包装

每个 `scn_data_index` entry 是 `{ offset: i32, size: i32 }`，offset 相对 `scn_data_list_ofs`。旧 C/C++ 代码把 scene data 视为：

```text
u32 comp_size
u32 decomp_size
u8  compressed_payload[comp_size]
```

`siglus_rs` 在 `original_source_header_size > 0` 时先对 chunk 做授权 decode，再 LZSS unpack，最后重建为连续的 decompressed scene data 区，并修正 index list。AstraEMU 只需要相同的内存形态：`ScenePck` 加载后能按 scene number 拿到 `.ss` chunk slice。

保护材料必须来自用户合法安装或项目配置，不写入 report，不进入 docs，不由 AstraEMU 提供提取工具。

## 资源格式

| 扩展名 | 作用 | 解析口径 |
| --- | --- | --- |
| `.g00` | 图像、UI、立绘、背景、mask | 首字节 type，后接 width/height；详见 [presentation-and-media.md](presentation-and-media.md) |
| `.g01` | 图像变体或成组资源 | 按 G00 family 资源处理，样本中常与 `.g00` 同名 |
| `.gan` | 动画描述 | 绑定 G00/layer/frame action，首阶段只做 presence 和 resolver |
| `.omv` | Siglus Ogg/Theora wrapper | header 后有内嵌 `OggS` bitstream |
| `.ovk` | Ogg/Vorbis pack | `u32 count` + 16-byte entry table |
| `.owp` | BGM Ogg wrapper | 不是裸 `OggS`，按授权 stream provider 解码 |
| `.ogg` | 裸 Ogg/Vorbis | 以 `OggS` 起始 |
| `.nwa` | 语音/音频 container | 44-byte NWA header，可压缩或未压缩 |
| `.mpg` | MPEG program stream | 扫描 sequence header `00 00 01 B3` |
| `.wmv` | Windows Media Video | 平台 decoder/provider |
| `.cgm` | CG table | `CGTABLE`/`CGTABLE2`，用于 CG/gallery |
| `.dbs` | database table | row/column/data/string table |
| `.tcr` | text/color/runtime config data | 当前只列入 resolver 和 diagnostics |

## OVK pack

OVK 文件以 `u32 entry_count` 起始，随后每个 entry 16 bytes：

```text
u32 size
u32 offset
u32 no
u32 sample_count
```

样本：

| 文件 | entry_count | entry[0] |
| --- | ---: | --- |
| `anemoi/.../koe/z0001.ovk` | 14 | `size=51021, offset=228, no=59, sample_count=157298` |
| `Rewrite_PLUS/koe/z1001.ovk` | 8 | `size=59236, offset=132, no=11, sample_count=94100` |

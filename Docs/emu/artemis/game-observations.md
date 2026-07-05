# Artemis Game Observations

## 记录边界

本页只保存本地结构化文件级观察。样本来自本地合法安装副本，统一写成 `<game-root>`。不记录绝对路径、剧情文本、完整脚本、素材内容、截图、音频或视频帧。

## サクラノ詩10th

### PFS

| 文件 | 格式 | 字节数 | entries | 主要内容 |
| --- | --- | ---: | ---: | --- |
| `sakuranouta10th.pfs` | PF8 | 1,409,676,961 | 1,823 | PNG、AST、JPG、OGG、HLSL、Lua、GLSL、IPT、IET、SLI、ASB |
| `sakuranouta10th.pfs.000` | PF8 | 1,627,584,931 | 17,615 | OGG、PNG、SLI |
| `sakuranouta10th.pfs.001` | PF8 | 1,414,608,400 | 17,951 | OGG、SLI |
| `sakuranouta10th.pfs.002` | PF8 | 1,740,039,732 | 800 | PNG |
| `sakuranouta10th.pfs.003` | PF8 | 759,668,861 | 367 | PNG |
| `sakuranouta10th.pfs.500` | PF8 | 1,100,732,980 | 2,995 | OGG、PNG、SLI、AST、JPG、TBL |

`.500` 符合官方三位 patch 规则，resolver 应让它覆盖 `.003`、`.002`、`.001`、`.000` 和根包中的同名 entry。

### `system.ini`

选中的 `WINDOWS` 段：

| Key | Value |
| --- | --- |
| `WIDTH` / `HEIGHT` | `1920` / `1080` |
| `CHARSET` | `UTF-8` |
| `BOOT` | `system/first.iet` |
| `RESIZABLE` | `1` |
| `FIXED_ASPECT_RATIO` | `1` |
| `SIDECUT` | `0` |
| `SAVEPATH_CSIDL` | `26` |
| `PREVENT_MULTIPLE_PROCESS` | `MakuraSakuranouta10th` |

移动和 WASM 段使用 1280x720，boot 同为 `system/first.iet`。

### Boot 和脚本

`system/first.iet` 大小 2,038 字节，解密后是文本脚本。tag 频次最高的是 `calllua`、`wt`、`lua`、`return`、`if`、`loading`、`stop`。可见 boot 函数名包括 `system_initlua`、`init_patch`、`system_starting`、`font_cache`、`brand_logo`、`title_cache`、`title_init`。

系统 ASB：

| 文件 | 大小 | magic | 可见 tag/string |
| --- | ---: | --- | --- |
| `system/script.asb` | 6,929 | `ASB\0` | `select`、`calllua`、`jump`、`return` |
| `system/ui.asb` | 6,090 | `ASB\0` | `calltag`、`exskip`、`uitrans`、`loading` |
| `system/save.asb` | 367 | `ASB\0` | `save`、`wait`、`calllua`、`return` |

Patch `.500` 中的 AST tag 名包括 `rt2`、`text`、`fg`、`vo`、`bg`、`se`、`bgm`、`extrans`、`quake`。这些名称用于 coverage，不复制参数值。

### Media

loose movie：

| 文件 | 字节数 | header |
| --- | ---: | --- |
| `e1r8fa9vg.wmv` | 674,844,703 | `30 26 b2 75` |
| `e275n3fcx.wmv` | 564,441,139 | `30 26 b2 75` |
| `ou9nq3hr2.wmv` | 235,152,211 | `30 26 b2 75` |

header 表明它们是 ASF/WMV 类文件。AstraEMU 只记录 magic 和大小，不保存帧。

## 终之空Remake2025

### PFS

| 文件 | 格式 | 字节数 | entries | 主要内容 |
| --- | --- | ---: | ---: | --- |
| `tsuinosora_remake2025ver.pfs` | PF8 | 796,020,279 | 10,919 | OGG、PNG、SLI、AST、Lua、ASB、IET、TBL |
| `tsuinosora_remake2025ver.pfs.000` | PF8 | 914,874,915 | 854 | PNG、IPT |
| `tsuinosora_remake2025ver.pfs.721.bak` | PF6 | 5,498,764 | 117 | AST、Lua、ASB、IET、TBL |

`.pfs.721.bak` 的内部格式是 PF6，但文件名不是官方 patch 形态。默认 probe 记录它，runtime resolver 不把它作为 `.721` patch 使用。

### `system.ini`

选中的 `WINDOWS` 段：

| Key | Value |
| --- | --- |
| `WIDTH` / `HEIGHT` | `1600` / `1200` |
| `CHARSET` | `UTF-8` |
| `BOOT` | `system/first.iet` |
| `RESIZABLE` | `1` |
| `FIXED_ASPECT_RATIO` | `1` |
| `SIDECUT` | `0` |
| `SAVEPATH_CSIDL` | `26` |

移动和 WASM 段使用 1280x720，boot 同为 `system/first.iet`。

### Boot 和脚本

`system/first.iet` 大小 2,012 字节。tag 频次最高的是 `calllua`、`wt`、`lua`、`return`、`if`、`loading`、`stop`。boot 函数名包括 `system_initlua`、`system_loadinglua`、`system_dataloading`、`init_patch`、`system_initialize`、`system_starting`、`brand_logo`、`title_cache`、`title_init`。

AST tag 名包括 `text`、`ruby`、`rt2`、`bg`、`cgdel`、`msg`、`se`、`extrans`、`bgm`、`msgoff`、`quake`、`vo`。这些只用于 coverage。

系统 ASB：

| 文件 | 大小 | magic | 可见 tag/string |
| --- | ---: | --- | --- |
| `system/script.asb` | 6,856 | `ASB\0` | `select`、`calllua`、`jump`、`return` |
| `system/ui.asb` | 5,979 | `ASB\0` | `calltag`、`exskip`、`uitrans`、`loading` |
| `system/save.asb` | 333 | `ASB\0` | `save`、`calllua`、`return` |

### Media

loose movie 是 `.dat` 文件，但 header 与 ASF/WMV 一致：

| 文件 | 字节数 | header |
| --- | ---: | --- |
| `brjzs892hzg1z.dat` | 2,701,583 | `30 26 b2 75` |
| `epjzs892hzg1z.dat` | 79,450,868 | `30 26 b2 75` |
| `epjzs892hzg2a.dat` | 231,971,240 | `30 26 b2 75` |
| `epjzs892hzg34.dat` | 511,175,948 | `30 26 b2 75` |
| `epjzs892hzg4z.dat` | 174,788,036 | `30 26 b2 75` |
| `epjzs892hzg5c.dat` | 398,972,035 | `30 26 b2 75` |
| `epjzs892hzg6d.dat` | 376,120,150 | `30 26 b2 75` |
| `epjzs892hzg72.dat` | 421,331,030 | `30 26 b2 75` |

## Cross-sample Conclusions

- `system.ini` boot 链稳定指向 `system/first.iet`。
- 系统脚本混用 `.iet`、`.asb`、`.lua`；正文大量使用 `.ast`。
- PF8 是主格式，但 PF6 仍可能出现在 backup 或兼容包中。
- 大视频不在 PFS 内，扩展名可能是 `.wmv` 或 `.dat`，必须看 magic。
- patch chain 不能只扫描连续编号；`.500` 是有效 patch。

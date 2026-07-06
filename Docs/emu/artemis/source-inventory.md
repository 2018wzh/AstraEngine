# Artemis Source Inventory

## 参考入口

| 来源 | 可用事实 | AstraEMU 用法 |
| --- | --- | --- |
| PFS 公开研究实现 | PF6/PF8 reader、writer、CLI、`system.ini` smart detection、path 转换和默认非加密扩展 | archive reader、probe、patch-chain resolver 和 synthetic fixture |
| Artemis spec 文档镜像 | `system.ini`、script syntax、pack file、macro、layer、system variables | boot contract、tag parser、变量模型、release gate 检查 |
| Artemis tag 文档镜像 | graphics、scenario、script、sound、system、var tag 参考 | tag executor vocabulary、PresentationCommand 和 AudioCommand 映射 |
| Artemis Lua 文档镜像 | `engine` Lua object、`setTagFilter`、`tag`、`enqueueTag`、输入和脚本状态 API | Lua sandbox allowlist、tag/Lua 互操作和 diagnostics |
| 本地样本「サクラノ詩10th」 | PF8 根包与 patch 包、`system.ini` boot、`.iet/.ast/.asb/.lua/.sli/.ipt/.tbl` 分布、loose WMV movie | file-level case report 和启动链验证 |
| 本地样本「终之空Remake2025」 | PF8 根包、PF8 patch、PF6 backup 包、`system.ini` boot、ASB/Lua 系统脚本、loose ASF/WMV movie | PF6/PF8 双格式 probe 和 patch-chain 边界验证 |

## pfs-rs 事实

| 路径 | 事实 |
| --- | --- |
| `pf8/src/format.rs` | magic 为 `pf6` 或 `pf8`；`index_size` 位于 offset `0x03`；`index_count` 位于 `0x07`；entry 从 `0x0B` 开始，包含 name length、name、4 字节保留、offset、size |
| `pf8/src/reader.rs` | reader 只把 header/index 读入内存，entry payload 按需 seek/read；PF6 不生成 key，PF8 从 index data 生成 key |
| `pf8/src/crypto.rs` | PF8 key 为 index data 的 SHA1；payload 使用 XOR，streaming 时按 payload offset 继续 key index |
| `pf8/src/entry.rs` | PF6 entry 视为未加密；PF8 默认对非过滤扩展加密；archive 内路径使用反斜杠，reader 转成平台路径 |
| `pf8/src/constants.rs` | 默认未加密扩展是 `mp4`、`flv` |
| `pfs-rs/src/commands.rs` | create 时如果单目录包含 `system.ini`，会把目录内容作为根结构打包 |
| `pfs-rs/src/util.rs` | `.pfs`、`.pfs.000` 这类文件名被识别为 PFS 输入；`system.ini` 只检查目录根 |

## 官方文档事实

| 文档 | 事实 |
| --- | --- |
| `spec/system_ini.md` | 引擎启动先读 `system.ini`；平台段按 `WINDOWS`、`IOS`、`ANDROID`、`WASM` 选择；共有键包含 `WIDTH`、`HEIGHT`、`SIDECUT`、`BOOT`、`CHARSET`、`NO_SAVE` |
| `spec/script_syntax.md` | 默认脚本是 Shift_JIS 文本，也可由 `CHARSET=UTF-8` 切到 UTF-8；非 `[]` 包围内容是剧情文本，`[]` 是 tag；变量前缀包含 `g.`、`t.`、`s.` |
| `spec/pack_file.md` | PFS 最大 2GB；Windows 根包优先同名 exe 的 `.pfs`，不存在时用 `root.pfs`；patch 文件从 `.pfs.000` 到 `.pfs.999` |
| `tag/readme.md` | tag 参数有 `STRING`、`NUMBER`、`PATH`、数组、颜色和缺省语义；推荐路径使用 `/` |
| `tag/system/lua.md` | `[lua]...[/lua]` 在文件加载时执行，不在 tag 运行到该位置时执行；所有文件共享同一个 Lua 环境 |
| `tag/system/calllua.md` | `calllua` 调 Lua 函数；Lua 函数第一个参数是 engine object，惯例命名为 `e` |
| `lua/engine/setTagFilter.txt` | Lua tag filter 可在 tag 执行前处理 tag，返回 0 继续原 tag，返回 1 跳过原 tag |
| `lua/engine/enqueueTag.txt` | `e:enqueueTag` 在当前 Lua 函数退出后、引擎进入下一个运行状态时按顺序执行 |
| `lua/engine/tag.txt` | `e:tag{...}` 立即执行 tag，参数必须是字符串 |

## 本地样本清单

样本名只用于本地结构化 case report，不记录本机绝对路径。

| 样本 | 文件 | 字节数 | 观察 |
| --- | --- | ---: | --- |
| サクラノ詩10th | `sakuranouta10th.pfs` | 1,409,676,961 | PF8，1,823 entries，含 `system.ini`、`system/first.iet`、`.ast/.asb/.lua/.ipt/.tbl` |
| サクラノ詩10th | `sakuranouta10th.pfs.000` | 1,627,584,931 | PF8，17,615 entries，主要是 voice OGG、PNG、SLI |
| サクラノ詩10th | `sakuranouta10th.pfs.001` | 1,414,608,400 | PF8，17,951 entries，主要是 OGG、SLI |
| サクラノ詩10th | `sakuranouta10th.pfs.002` | 1,740,039,732 | PF8，800 PNG entries |
| サクラノ詩10th | `sakuranouta10th.pfs.003` | 759,668,861 | PF8，367 PNG entries |
| サクラノ詩10th | `sakuranouta10th.pfs.500` | 1,100,732,980 | PF8，2,995 entries，含 patch AST、table、media |
| サクラノ詩10th | `movie/*.wmv` | 235MB 到 675MB | loose WMV/ASF，header `30 26 b2 75` |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs` | 796,020,279 | PF8，10,919 entries，含 `system.ini`、`system/first.iet`、`.ast/.asb/.lua/.tbl` |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs.000` | 914,874,915 | PF8，854 entries，主要是 PNG 和一个 IPT |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs.721.bak` | 5,498,764 | PF6，117 entries，是 backup 形态，不按官方 patch 文件名直接进入 runtime chain |
| 终之空Remake2025 | `movie/*.dat` | 2.7MB 到 512MB | loose ASF/WMV，header `30 26 b2 75` |

## 不进入 AstraEMU core 的内容

`*.exe`、`*.dll`、环境信息脚本、卸载脚本、商业补丁包和媒体 payload 不属于 family plugin 输入。Artemis plugin 只需要合法安装后的 data root、PFS/loose resolver、脚本 metadata、可解码 media block 引用和用户授权的 save root。

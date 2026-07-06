# Source Inventory

本页说明 KrKr 文档用到的证据，不记录商业内容本身。

## 3lj 样本

样本根目录是一个已打包 KrKr 游戏目录。根目录包含 18 个 `.xp3`、一个 `plugin/` 目录、一个 `savedata/` 目录、一个 Kirikiroid2 偏好 XML、一个 standalone `patch.tjs`，另有一个全 CG 存档目录。`patch.tjs` 的文件头是 `TJS2100`，按编译后的 TJS2 bytecode 处理，不按 UTF-16 source 处理。

| 文件 | 大小级别 | index 条目 | 主要内容 |
| --- | ---: | ---: | --- |
| `data.xp3` | 538 MB | 1,886 | TJS、KAG `.ks`、PSB、PNG/JPG/OGG/UI 配置 |
| `scn.xp3` | 12 MB | 138 | `.ks.scn` 场景 |
| `patch.xp3` | 47 MB | 1,023 | 场景、立绘、UI、音频和 TJS 覆盖 |
| `patchAI.xp3` | 22 MB | 177 | unencrypted TJS source、`.ks.scn`、字体和文本 |
| `yuzu_0012t_ai (1).xp3` | 22 MB | 203 | `patchAI` 与 UI TLG 的合并形态 |
| `voice.xp3` | 984 MB | 48,314 | `.ogg`、`.sli`、少量 voice metadata |
| `voice2.xp3` | 105 MB | 7,092 | 追加 voice `.ogg` 与 `.sli` |
| `bgm.xp3` | 333 MB | 198 | `.opus`、`.ogg`、`.sli`、`.mchx` |
| `video.xp3` | 644 MB | 9 | `.wmv` 与 `.mp4` |
| `fgimage.xp3` | 67 MB | 1,256 | `.tlg`、`.pbd`、`.sinfo`、stand metadata |
| `bgimage.xp3` | 86 MB | 229 | background PNG 与 `base.stage` |
| `evimage*.xp3` | 191 MB | 688 | event image `.pimg` 与 PNG |
| `image.xp3` | 46 MB | 102 | system UI PNG/PBD/ASD |
| `uipsd.xp3` | 21 MB | 170 | UI TLG/PBD/FUNC |
| `patch2.xp3` | 711 KB | 26 | UI TLG 覆盖 |
| `LLLpatch.xp3` | 941 KB | 101 | PBD/PNG/TOML 覆盖 |
| `patchAI_UI.xp3` | 5 MB | 3 | `ctxfontprefs.tjs`、`simhei.ttf`、`uitexts.toml` |

`savedata/` 中观察到 `.ksd`、`.cfu` 和 `savecheck`。这些文件只用于确认 KrKr 存档形态存在，不进入 AstraEMU public save format。

## 插件目录

样本 `plugin/` 有 39 个 DLL。按能力粗分：

| 能力 | 文件例子 |
| --- | --- |
| 渲染和窗口 | `DrawDeviceD2D.dll`、`SteamDrawDevice.dll`、`windowEx.dll` |
| layer/filter/text | `layerExDraw.dll`、`layerStwCopy.dll`、`textrender.dll`、`GlitchEffect.dll` |
| audio | `wuvorbis.dll`、`wuopus.dll`、`wvdecoder.dll`、`wumultitrack.dll`、`wfBasicEffect.dll`、`wfTypicalDSP.dll` |
| movie/motion | `krmovie.dll`、`AlphaMovie.dll`、`motionplayer.dll`、`motionplayer_nod3d.dll` |
| data/format | `json.dll`、`toml.dll`、`psbfile.dll`、`psd.dll`、`lzfs.dll` |
| platform/service | `krkrsteam.dll`、`dmmcloud.dll`、`httprequest.dll`、`win32dialog.dll`、`win32ole.dll` |
| KAG compatibility | `k2compat.dll`、`kagexopt.dll`、`KAGParserEx.dll` |

AstraEMU 需要把这些 DLL 识别成 capability requirement。Manager 不直接加载旧 DLL；需要加载时也只能发生在 KrKr provider session 的 capability sandbox 内。

## FuckGalEngine/Krkr 参考

| 参考文件 | 采用的事实 |
| --- | --- |
| `XP3Viewer-121113/krkr2.h`、`krkr2.cpp` | XP3 magic、`File/info/segm/adlr/time` chunk、segment zlib flag、UTF-16 文件名、Adler-32 |
| `Krkr_text_out*.py`、`Krkr_text_in.py` | KAG 文本行、`@` command、`[]` tag、`*` label 和 UTF-16/CP932 脚本处理经验 |
| `tools-of-Amaf/常用脚本/Conductor.tjs` | KAG conductor 的 tag handler、wait、trigger、timeout 执行模型 |
| `M2Psb/PSBReader*`、`psbinfo.h` | PSB header、name tree、string table、DIB/resource table 的字段关系 |
| `脚本修改.txt` | `Plugins.link(...)` 可在 TJS 层注入插件能力；该材料只作为兼容观察，不作为补丁流程 |

`KrkrSrcDecrypt.asm`、KRPatch、截图和 hook 笔记只说明生态里存在补丁/调试做法。本规格不采用其中的绕过步骤，也不把它们写成产品能力。

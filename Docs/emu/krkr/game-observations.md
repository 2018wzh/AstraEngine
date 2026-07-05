# Game Observations

本页记录 3lj 样本的本地结构化事实。只保留结构、文件名、计数、flag 和格式判断。

## 根目录形态

样本根目录由 base archive、patch archive、plugin、savedata 和 standalone patch 组成：

- base archive：`data.xp3`、`scn.xp3`、`bgimage.xp3`、`fgimage.xp3`、`image.xp3`、`uipsd.xp3`、`bgm.xp3`、`voice.xp3`、`voice2.xp3`、`video.xp3`、`evimage.xp3`、`evimage2.xp3`。
- patch archive：`patch.xp3`、`patch2.xp3`、`LLLpatch.xp3`、`patchAI_UI.xp3`、`patchAI.xp3`、`yuzu_0012t_ai (1).xp3`。
- standalone script：`patch.tjs`，magic 为 `TJS2100`。
- plugin：39 个 DLL。
- savedata：`.ksd`、`.cfu`、`savecheck`。

## Script Facts

`data.xp3` 同时包含 TJS source、KAG `.ks` 和 PSB/resource 文件。可见的 KAG/TJS storage 包括：

- `appconfig.tjs`
- `main\default.tjs`
- `main\config.tjs`
- `main\custom.ks`
- `main\sysmenu.ks`
- `scenario\start.ks`
- `scenario\macro.ks`
- `scenario\replay.ks`
- `scenario\transitions\trans_crossfade.tjs`

`scn.xp3` 有 138 个 `.ks.scn`，例如 `バンド001_03月_プロローグ上（現状）.ks.scn`、`バンド002_03月_プロローグ下（ライブ）.ks.scn`、`月望_01.ks.scn`。`patchAI.xp3` 对这些 `.ks.scn` 提供未加密 flags 的覆盖版本，payload 头部为 `PSB\0`。

`patchAI.xp3` 中的 `default.tjs` 是 UTF-16 source。结构上能看到：

- `CUSTOM_USE_MP4`
- `CUSTOM_USE_MOTION`
- `MovieAudioSampleFile`
- BGM、voice、movie volume 配置
- `KAGLoadScript("yuzu_default.tjs")`
- `SystemConfig.routeScenarioPrefixMap`

这些只说明 boot 和配置路径，不复制脚本文本。

## XP3 Flags

已解析 archive 的 index 都能读出 `File/info/segm/adlr` 结构。多数条目 `info.flags` 是 `0x80000000`，`patchAI.xp3` 和 `patch2.xp3` 的条目是 `0`。所有已解析 segment 的 flag 是 `1`，即 zlib segment。

index record 有压缩和 raw 两种形态。`patchAI.xp3`、`patch2.xp3` 使用 raw index；其他大多数 archive 使用 zlib index。

## Patch Coverage

样本不是单一 base 包。覆盖关系很重：

- `patch.xp3` 覆盖了 125 个 base `.ks.scn`。
- `patchAI.xp3` 覆盖了 138 个 base `.ks.scn`，还覆盖 `appconfig.tjs` 和多份 TJS。
- `yuzu_0012t_ai (1).xp3` 覆盖范围更大，像一个整合 patch。
- `patch2.xp3` 只覆盖 UI TLG。
- `LLLpatch.xp3` 主要覆盖 PBD、PNG、TOML。

因此 release gate 必须验证 layer order。只验证 archive 能打开，不足以说明游戏会按预期运行。

## Media Facts

- `voice.xp3` 有 48,314 个条目，其中 `.ogg` 与 `.sli` 最多。
- `voice2.xp3` 有 7,092 个条目，主要是追加 voice。
- `bgm.xp3` 使用 `.opus`、`.ogg`、`.sli`、`.mchx`。
- `video.xp3` 同时包含 `.wmv` 和 `.mp4`。
- UI 和图像覆盖大量使用 `.tlg`、`.pbd`、`.pimg`、`.psb`、`.stage`。

插件目录与这些媒体事实匹配：`wuopus`、`wuvorbis`、`wvdecoder`、`krmovie`、`AlphaMovie`、`motionplayer`、`textrender` 等都存在。

## 风险

- `.ks.scn` 是 binary scenario，不能靠文本 KAG parser 完成兼容。
- `TJS2100` bytecode 需要 TJS VM 或 bytecode loader。
- 旧插件数量多，必须先做 capability map。
- patch layer 多，错误顺序会让脚本和 UI 回退到旧版本。
- savedata 是 KrKr 私有形态，不能直接当作 Astra save。

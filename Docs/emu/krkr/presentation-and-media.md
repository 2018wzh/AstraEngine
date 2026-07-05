# Presentation And Media

KrKr 的 presentation 由 layer、message layer、transition、movie 和 audio graph 共同组成。AstraEMU 只接收规范化 command；旧插件和旧 renderer state 留在 KrKr compat core。

## 图像和 UI

3lj 样本中观察到：

| 扩展 | 位置 | 说明 |
| --- | --- | --- |
| `.png`、`.jpg` | `data.xp3`、`bgimage.xp3`、`image.xp3`、patch archive | UI、背景、缩略图 |
| `.tlg` | `fgimage.xp3`、`uipsd.xp3`、patch archive | KrKr 常见图像格式 |
| `.pimg` | `evimage*.xp3` | event image 容器 |
| `.pbd`、`.sinfo`、`.stand` | `fgimage.xp3`、`uipsd.xp3`、patch archive | 立绘/UI layout metadata |
| `.psb` | `data.xp3` | PSB resource |
| `.stage` | `bgimage.xp3`、`patch.xp3` | stage metadata |

`XP3Viewer` 参考代码包含 TLG decoder。AstraEMU 可以用 provider 解 TLG，但 provider 输出只能是 media block 或 decoded image，不跨 ABI 传递旧 engine layer 对象。

## Layer 和 Transition

KAG 常见动作：

- 背景 layer 切换。
- foreground/standing layer 更新。
- message layer 文本绘制。
- UI layer 和 popup。
- transition script，例如 `scenario/transitions/trans_crossfade.tjs`、`trans_blurfade.tjs`。

Core 输出的 command 应表达“目标 layer、资源、矩形、opacity、blend、transition”，不要暴露 TJS object pointer。无法精确复现的 transition 先输出 diagnostic 和近似 command。

## Audio

样本音频集中在：

- `bgm.xp3`：`.opus`、`.ogg`、`.sli`、`.mchx`。
- `voice.xp3`：29,580 个 `.ogg`、18,683 个 `.sli`。
- `voice2.xp3`：4,172 个 `.ogg`、2,920 个 `.sli`。

插件目录中有 `wuopus.dll`、`wuvorbis.dll`、`wvdecoder.dll`、`wumultitrack.dll`、`wfBasicEffect.dll`、`wfTypicalDSP.dll`。这些说明 runtime 需要 codec、loop/timing、voice replay、多轨和 DSP capability。AstraEMU 第一阶段可以只输出 audio command 和 capability requirement，实际 decode 可由 platform provider 或 FFmpeg fallback 完成。

`.sli` 应按 sidecar timing/loop metadata 处理，不是独立音频流。

## Movie

`video.xp3` 有 8 个 `.wmv` 和 1 个 `.mp4`。`default.tjs` source 中可见 `CUSTOM_USE_MP4`、`MovieAudioSampleFile`、movie volume 和 movie skip 相关配置。插件目录中有 `krmovie.dll`、`AlphaMovie.dll`、`motionplayer.dll`、`motionplayer_nod3d.dll`。

Core 输出 movie command 时要记录：

- requested storage。
- resolved file 和 archive layer。
- container/codec guess。
- audio route。
- skip policy。
- completion wait token。

Manager 选择平台 decoder。桌面可以用 platform decode 或 FFmpeg fallback；旧 movie DLL 不进入 EngineCore。

## Text

文本相关插件和资源包括 `textrender.dll`、`.tft` 字体、`ctxfontprefs.tjs`、`ctxfontprefs.toml`、`simhei.ttf`。message layer 输出要保留 font face、size、ruby、shadow/edge、name window 和 text speed 之类参数，但 TextCaptureEvent 不提交商业正文。

## Kirikiroid2 Preference

样本根目录有 Kirikiroid2 preference XML，记录 `renderer=opengl`、`fps_limit=90`、`force_default_font=1`、`outputlog=1`。这只说明样本被移动端 KrKr 兼容层运行过；AstraEMU 不继承 Kirikiroid2 配置格式，只把它作为 probe metadata。

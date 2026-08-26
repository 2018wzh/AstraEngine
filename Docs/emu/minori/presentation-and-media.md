# Minori Presentation And Media

## 资源分区

| Role | Archive | Runtime 命令 |
| --- | --- | --- |
| 背景/立绘/事件图 | `st.paz` | `SetBackground`, `ShowSprite`, `MoveSprite` |
| UI/system | `sys.paz` | message window、config、save/load UI |
| SE | `se.paz` | `PlaySe` |
| Voice | `voice.paz` | `PlayVoice` |
| Movie | `mov.paz` 或 loose | `PlayMovie` |

## Layer Model

AstraEMU Minori core 用固定 layer：

```text
background
event
character slots
effects
message window
system overlay
```

每个 layer command 记录资源名、slot、坐标、alpha、transition、duration 和原始 opcode offset。

## Text

消息正文不进入可序列化 presentation DTO。Family 在 session 内保留有界的一次性文字 capture，用 `LegacyTextPresentationV1` 传递脱敏布局：`ja-JP`、显式 Noto Sans JP、body/speaker region、字号、行高、行数和 RGBA；该内部 capture 不跨 ABI。ABI v9 主路径先调用同步 translation Hook，再由 family-owned `CosmicTextLayoutProvider` 和 `astra-media-core` 的 CPU Renderer2D 生成 text surface，最后作为 retained `Layer2D` layer 提交；glyph resource 和合成像素只在当前运行中存在。

当前参考 stage 固定为 1280×720，原程序默认正文字号已由反编译确认是 26 px，ruby 为 12 px。body/speaker 的区域坐标结合已确认 panel 几何与外部截图结构建立。真实 Headless 首条 message、Config、backlog 和 gallery checkpoint 已人工检查：日文字形完整可读，没有缺字方框、横向裁剪或拉伸，正文位于 panel 有效区域。该结果只构成当前布局的 E2 视觉证据，不是原版像素 parity。缺少精确 stage、字体或 provider 时直接返回稳定 diagnostic，不读取系统字体，也不切换到私有文字 rasterizer。

## Audio

BGM、SE、voice 分离。Voice replay 从 backlog 触发时不能推进脚本 VM；只提交 `AudioCommand::PlayVoiceReplay`。

## Movie

当前样本 `mov.paz` 非空并含 5 个 entry。VFS 负责准确解密和读取；`PlayMovie` 在 Headless、Manager 和 CLI 的 Minori 路径都绑定 AstraMedia 的 `ffmpeg-vcpkg` 增量 provider，以有界 `Read + Seek` 源逐包产生单调 PTS、BGRA 帧和交错 PCM，再交给公共 `IncrementalMediaPlayback`、media fence、Renderer2D 和 Kira 音频队列。游标同时执行 tick、packet、视频 lead/lag 和迟到策略边界，`Drop` 仅在 profile 明确允许时计数，`Block` 直接阻断。Manager VFS viewer 的 `.avi` 首帧同样要求显式 FFmpeg binding；没有 FFmpeg 或 provider binding 不匹配时直接返回稳定 diagnostic，不调用系统 codec、手写 AVI/WMV 解码器或 fallback。movie gallery 的脚本目标和标签已校验，但样本没有独立的 gallery 背景资源，因此当前页面保留严格有界的已验证 Memories 背景近似，不能作为原版逐像素 parity 证据。

当前 Family ABI v9 的 `LegacyAudioCommandV1::Play` 与 `LegacyVideoCommandV1::Play` 没有起始 PTS 或 seek 字段。Minori snapshot 会保存活动资源、编码、循环和 continuation marker，并在 restore 时重新校验资源 identity；Host 只能按公共 ABI 重新提交从起点开始的 `Play`，不能把 marker 冒充成可寻址的媒体续播。需要原位置恢复的音频/影片 continuation 是明确的 blocking 项，必须先由 ABI/Host 提供经过验证的 seek contract，再补实现和 evidence；当前不使用平台播放器、私有 decoder 或伪造 completion 绕过该边界。

## `bg` / `bgm` 真实 inventory

八包 full verify 后，`census-media` 对 `bg`、`bgm` 做了 payload-free 格式核验：

| 格式 | Entry | Frame | 验证路径 |
| --- | ---: | ---: | --- |
| PNG | 2655 | 2655 | workspace `image` provider |
| ANI | 1951 | 6723 | GARbro contract 对应的纯 Rust 有界 adapter，输出 `image::RgbaImage` |
| SQZ1 | 9 | 224 | 有界 zlib + BGRA32 adapter，逐 frame 校验精确输出大小 |
| Ogg | 49 | 49 streams | `OggS` signature；实际播放仍需 Astra Symphonia binding |
| metadata database | 1 | 不适用 | 只计数，不按图像或音频猜测 |

本轮共读取 4183190587 decoded bytes，验证的图像 frame 合计 2977549990 pixels，最大观测尺寸为 3840×3600。这个 census 证明 container 与像素转换可读，不证明 Renderer2D 合成、音频播放或视觉 parity。

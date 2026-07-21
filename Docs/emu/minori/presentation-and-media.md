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

消息正文不进入可序列化 presentation DTO。Family 通过一次性 lease 把正文交给 Host，并用 `LegacyTextPresentationV1` 传递脱敏布局：`ja-JP`、显式 Noto Sans JP、body/speaker region、字号、行高、行数和 RGBA。Host 复用 `CosmicTextLayoutProvider`、`TextRenderResourceOwner` 与 `astra-media-core` 的 CPU Renderer2D；glyph resource 和合成像素只在当前运行中存在。

当前参考 stage 固定为 1280×720，原程序默认正文字号已由反编译确认是 26 px，ruby 为 12 px。body/speaker 的区域坐标结合已确认 panel 几何与外部截图结构建立。真实 Headless 首条 message checkpoint 已人工检查：日文字形完整可读，没有缺字方框、横向裁剪或拉伸，正文位于 panel 有效区域。该结果只构成当前布局的 E2 视觉证据，不是原版像素 parity。缺少精确 stage、字体或 provider 时直接返回稳定 diagnostic，不读取系统字体，也不切换到私有文字 rasterizer。

## Audio

BGM、SE、voice 分离。Voice replay 从 backlog 触发时不能推进脚本 VM；只提交 `AudioCommand::PlayVoiceReplay`。

## Movie

当前样本 `mov.paz` 非空并含 5 个 entry。VFS 只负责准确解密和读取；`PlayMovie` command、媒体解码、时间轴与缺失资源策略属于下一阶段 runtime/media 接入，不能由 archive 可读性推断完成。

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

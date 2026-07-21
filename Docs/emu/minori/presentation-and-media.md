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

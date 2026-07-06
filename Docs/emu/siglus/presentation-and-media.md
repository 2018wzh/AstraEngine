# Siglus Presentation And Media

Siglus presentation 由脚本命令驱动，核心对象是 stage、object、message window、button/select item、screen effect、audio/movie channel。AstraEMU family plugin 负责把这些 legacy 状态转成 Runtime presentation/audio event，Manager 不直接解释 Siglus layer 内存。

## G00 图像

G00 header 起始：

```text
u8  type
u16 width
u16 height
```

参考实现支持四类：

| type | 数据 | 说明 |
| ---: | --- | --- |
| 0 | LZSS32 compressed 32bpp pixels | 常见背景；输出 BGRA/RGBA |
| 1 | LZSS palette + indices | 8-bit indexed image |
| 2 | cut/chip directory + LZSS payload | UI、立绘部件、mask 等多 cut 资源 |
| 3 | JPEG bitstream | 仍以 G00 header 包装 |

样本 header：

| 文件 | type | width | height | size |
| --- | ---: | ---: | ---: | ---: |
| `anemoi/.../g00/__face_mask.g00` | 2 | 1920 | 1080 | 200,641 |
| `Rewrite_PLUS/g00/BG001.g00` | 0 | 1280 | 720 | 2,012,485 |

Type2 cut 数据包含 cut header、chip header 和 raw BGRA chip。AstraEMU 第一阶段不需要导出图片文件，只要能按资源名加载成 renderable texture，并在 diagnostics 中报告 type、size、decode status。

## Layer 和 wipe

参考 VM 的 `CommandContext` 包含 `LayerManager`、`GfxRuntime`、`UiRuntime`、tonecurve、wipe front/next render target 和 overlay runtime image。AstraEMU 应输出稳定的 presentation 命令：

```text
LoadImage(resource_name, g00_cut)
SetSprite(stage, object, layer, order, transform, alpha, clip)
SetTextWindow(mwnd, text_hash, name_hash, style_ref)
BeginWipe(kind, duration, mask_ref, affected_range)
EndWipe
```

Mask wipe 使用 mask image luminance/alpha 作为阈值。Mosaic、blur、tonecurve 等高阶 effect 先映射成 feature flags；没有 provider 时允许降级为 alpha fade，并在 report 标记 `DONE_WITH_CONCERNS`。

## OMV 视频

OMV 是 Siglus 的 Ogg/Theora wrapper。header 固定字段：

| offset | 字段 |
| ---: | --- |
| `0x00` | `u32 header_size` |
| `0x04` | `u32 version` |
| `0x28` | `u32 theora_type`，0 RGB、1 RGBA、2 YUV |
| `0x2C` | `u32 display_width` |
| `0x30` | `u32 display_height` |
| `0x3C` | `u32 frame_time_us` |
| `0x40` | `u32 max_data_size` |
| `0x4C` | `u32 page_count_hint` |
| `0x50` | `u32 packet_count_hint` |

样本：

| 文件 | header_size | version | type | display | frame_time_us | `OggS` offset |
| --- | ---: | ---: | ---: | --- | ---: | ---: |
| `anemoi/.../mov/ef_aurora_slow.omv` | 168 | 257 | 1 | 1920x1080 | 33333 | 32908 |
| `Rewrite_PLUS/mov/ef_ak_da_aura00.omv` | 168 | 257 | 1 | 500x460 | 33333 | 5940 |

## 音频

| 格式 | 观测 | Runtime 口径 |
| --- | --- | --- |
| `.ogg` | 裸 Ogg/Vorbis，首 4 bytes 为 `OggS` | 直接交给 Vorbis provider |
| `.owp` | BGM wrapper，不以 `OggS` 起始 | 通过授权 stream provider 变成 Ogg/Vorbis |
| `.ovk` | 语音 pack，`u32 count` + entry table | 按 entry number 建立 voice clip |
| `.nwa` | 44-byte header，可未压缩或压缩 | 输出 PCM16/PCM8 stream |

样本：

| 文件 | 观测 |
| --- | --- |
| `Rewrite_PLUS/bgm/BGM001.ogg` | header `4f 67 67 53`，即 `OggS` |
| `anemoi/.../bgm/M01A.owp` | header `76 5e 5e 6a ...`，不是裸 Ogg |
| `Rewrite_PLUS/koe/2036/Z203600522.nwa` | stereo、16-bit、44100 Hz、`pack_mod=-1`、`sample_cnt=155700` |

OWP 的具体 transform 不写入文档。Family plugin 只声明“需要合法资源 provider 解码”。

## MPEG/WMV

Rewrite_PLUS 同时有 `.mpg` 和 `.wmv`。MPEG 可用 sequence header `00 00 01 B3` 取基本信息；样本 `Rewrite_PLUS/mov/op01.mpg` 在 offset 2078 处有 1280x720，`frame_rate_code=1`。WMV 交给平台 decoder。

## Provider 输出

Plugin 不把原始像素或音频塞进普通 event。大块媒体走 content-addressed media block，provider event 只传：

```text
resource_id, decoded_format, width/height or sample_rate/channels, frame_no, hash, diagnostics
```

需要保存画面证据时，Release Gate 生成本地结构化 hash 和小尺寸内部截图；不提交未授权截图。

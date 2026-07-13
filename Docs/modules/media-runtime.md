# Media Runtime Module

Media Runtime 执行表现，不保存剧情权威状态。它消费 PresentationCommand、AudioCommand、TimelineCommand 和 FilterGraph/AudioGraph source。

## Provider Slots

- `astra.renderer2d`
- `astra.text_layout`
- `astra.audio_output`
- `astra.image_decode`
- `astra.audio_decode`
- `astra.video_decode`
- `astra.filter_node`
- `astra.audio_node`

## 内置 provider

- Renderer2D：真实 wgpu owner 位于 platform host 的 `WgpuPresentationCore`；Windows 路径覆盖 hardware surface、ordered frame、resize、context/device loss、retained frame resource rebuild 和 readback。Headless capture 只作为 reference provider；Migration 11 planned 完整 CPU reference path。
- TextLayout：`astra.text_layout.v2` 的 cosmic-text/Swash provider；只装载 target/profile 允许且 hash/face/coverage 可验证的 package 字体，输出 shaped glyph、cluster/font identity 与 Alpha8/RGBA glyph resource。`TextRenderResourceOwner` 管理跨 frame upload/reference/release，不能由 Player 自行估算字符矩形。
- Decode：`DecodeBindingContext` 精确绑定 provider/target/profile；fallback 和 reference provider 都要显式授权。Windows WMF provider 已用 CC0 public MP3/MP4 fixture 验证 bounded PCM、BGRA 首帧、corrupt input diagnostic 和失败后 sequence retry。`ffmpeg-vcpkg` provider 已覆盖 native probe、真实 MP3 decode/resample、真实 MP4 first-frame 及 corrupt container blocking；seek/pause/resume/EOS session 与 A/V sync 尚未闭合，不能据 one-shot evidence 宣称完整 release fallback。
- Audio：`astra.audio_graph.v2` 提供事务性 voice/fade 生命周期和 fixed-delta continuation；真实 Windows output 使用 bounded queue、WASAPI callback meter、错误传播和 close drain，不存在用 graph hash 冒充 meter 的产品 provider。Migration 11 planned 完整 PCM WAV output。

## 测试

Media tests 覆盖 `astra.font_manifest.v1` 经 package VFS context 解析真实字体 section、packaged font hash/face/coverage、Latin/组合字符、ruby、BiDi paragraph、真实 glyph raster、wrap/clip/ellipsis、动态字体 cache invalidation、transactional renderer resource lifecycle、显式 decode binding、corrupt/truncated input、AudioGraph voice/fade/loop/seek/transaction、FilterGraph unknown/no-op/target bypass、Windows ordered frame/resize/readback、WASAPI queue/drain、WMF audio/video decode 和 optional FFmpeg 真实 one-shot decode。CJK/Arabic/emoji licensed fixture、Windows glyph visual golden、FFmpeg session、A/V sync 和性能 evidence 尚未闭合。

这些测试只证明 Media contract 与局部 provider。完整 Headless Platform、全 Runtime test 收束、真实 PNG/WAV 和模型审查见 [Migration 11](../migrations/headless-platform-test-backend-migration.md)，当前状态为 `SPEC_READY`。

## Runtime 边界

Media Runtime 只消费 command，不写 VN route、backlog、read-state 或 save authority。视频、音频、滤镜等待点必须回到 AwaitToken/Fence，不能通过 provider callback 改 Runtime state。实现 trait、默认 provider 和 gate 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

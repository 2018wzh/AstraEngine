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

- Renderer2D：wgpu provider，当前 headless capture provider；Migration 11 planned 完整 CPU reference path。
- TextLayout：cosmic-text/Swash provider。
- Decode：platform provider 优先，desktop FFmpeg fallback。Windows WMF provider 已用 CC0 public MP3/MP4 fixture 验证 bounded PCM 和 BGRA 首帧；Web browser media path 用同一 fixture 验证 MP4/WebM/MP3 metadata load。
- Audio：platform output provider + 当前 headless meter provider；Migration 11 planned 完整 PCM WAV output。

## 测试

Media tests 覆盖 image/font/text/filter/audio decode、headless capture、AudioGraph bus、FilterGraph typed node validation、public media manifest hash、WMF audio/video decode 和 provider fallback。视频 fallback 通过 optional `ffmpeg-vcpkg` feature 接入；默认 workspace build 不要求本机 FFmpeg。

这些测试只证明 Media contract 与局部 provider。完整 Headless Platform、全 Runtime test 收束、真实 PNG/WAV 和模型审查见 [Migration 11](../migrations/headless-platform-test-backend-migration.md)，当前状态为 `SPEC_READY`。

## Runtime 边界

Media Runtime 只消费 command，不写 VN route、backlog、read-state 或 save authority。视频、音频、滤镜等待点必须回到 AwaitToken/Fence，不能通过 provider callback 改 Runtime state。实现 trait、默认 provider 和 gate 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

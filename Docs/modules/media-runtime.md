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

- Renderer2D：wgpu provider，headless capture provider。
- TextLayout：cosmic-text/Swash provider。
- Decode：platform provider 优先，desktop FFmpeg fallback。
- Audio：platform output provider + headless meter provider。

## 测试

Media tests 必须覆盖 image/font/text/filter/audio/video decode、headless capture、AudioGraph bus、FilterGraph typed node validation 和 provider fallback。

## Runtime 边界

Media Runtime 只消费 command，不写 VN route、backlog、read-state 或 save authority。视频、音频、滤镜等待点必须回到 AwaitToken/Fence，不能通过 provider callback 改 Runtime state。实现 trait、默认 provider 和 gate 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

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

## 最小内置 provider

- Renderer2D：wgpu provider，headless capture provider。
- TextLayout：cosmic-text/Swash provider。
- Decode：platform provider 优先，desktop FFmpeg fallback。
- Audio：platform output provider + headless meter provider。

## 测试

Media tests 必须覆盖 image/font/text/filter/audio/video decode、headless capture、AudioGraph bus、FilterGraph typed node validation 和 provider fallback。

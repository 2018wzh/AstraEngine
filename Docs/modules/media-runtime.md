# Media Runtime Module

Media Runtime 执行表现，不保存剧情权威状态。它消费 PresentationCommand、AudioServiceCommand、TimelineCommand 和 FilterGraph source。

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

### RFVP streaming decode

Windows WMF now exposes bounded video and audio cursors through PlatformHost.
The first request establishes the cursor and subsequent requests transfer one
validated frame/chunk. A bounded worker performs decode and the runtime only
consumes ready data. The host reports `ASTRA_MEDIA_STREAM_EOS` instead of
requiring a polling timeout. Hardware transforms are selected when available;
the contract deliberately records the CPU BGRA/i16 output boundary and the
single final upload/device transfer.

- Renderer2D：真实 wgpu owner 位于 platform host 的 `WgpuPresentationCore`；Windows 路径覆盖 hardware surface、ordered frame、resize、context/device loss、retained frame resource rebuild 和 readback。文本 pass 通过 `surface.present_text_scene` 执行真实 GPU atlas、vertex/scissor draw 和事务性资源提交；非文本 `SceneCommand` 仍明确拒绝，不能把该 pass 外推成完整 GPU renderer。Migration 11 Headless 组合 Media-owned CPU provider 并写出 lossless PNG。
- TextLayout：`astra.text_layout.v2` 的 cosmic-text/Swash provider；只装载 target/profile 允许且 hash/face/coverage 可验证的 package 字体，输出 shaped glyph、cluster/font identity 与 Alpha8/RGBA glyph resource。普通 cache 使用 typed request key、font generation 与 layout revision，不序列化或计算 layout content hash。`TextRenderResourceOwner` 管理跨 frame upload/reference/release；`astra.text_layout_replay.v1` 在独立 persisted 边界固化 bounded provider/font/layout/glyph record。Windows hardware glyph consumer 读取相同 command stream，visual golden digest 在 GPU submit/capture 后由 Evidence consumer 生成；失败 present 不提交逻辑资源，loss 后从 retained bitmap 重建。Player 不能自行估算字符矩形，也不能在 replay 时重新调用 live font provider。
- Decode：`DecodeBindingContext` 精确绑定唯一 provider/target/profile；reference provider 仅用于 non-packaged 测试。Windows WMF provider 已用 CC0 public MP3/MP4 fixture 验证 bounded PCM、typed BGRA、corrupt input diagnostic 和失败后 sequence retry。`ffmpeg-vcpkg` 是可单独选择的 provider，不作为 WMF 失败后的 fallback。Headless 完整视频使用 typed descriptor/frame/end 输出，避免跨 Player boundary 聚合全部 BGRA frame。
- Audio：`astra-audio-kira` 持有 voice、bus、fade、loop、seek、clock 与混音，`AudioOutputLane` 将 owned PCM chunk 移交平台 endpoint；callback 只消费 chunk 与 offset。Headless 使用同一 Kira 服务和 deterministic endpoint 生成 WAV，Windows output 使用独立 WASAPI lane、callback meter、错误传播和 close drain。
- MediaPlayback：`astra.media_playback.v1` 以 audio callback playhead 为 A/V master clock，管理 timestamped audio/video packet、bounded queue、play/pause/seek/EOS/cancel、显式 late-video policy、事务 tick 和 validated snapshot restore。它是共享 scheduler，不替代真实 decode-to-output 接线。

## 测试

Media tests 覆盖 package VFS font、真实 glyph shaping/raster、transactional scene resource lifecycle、唯一 decode binding、corrupt/truncated input、Kira voice/bus/fade/loop/owned chunk、MediaPlayback audio-master clock/seek/EOS/rollback/snapshot、FilterGraph blocker、Windows ordered frame/resize/readback、WASAPI queue/drain 与 WMF typed audio/video decode。`ffmpeg-vcpkg` feature 需在具备本机 FFmpeg 的环境单独复核。Web consumer、完整产品 camera/timeline/video/audio、GPU FilterGraph 与正式性能证据尚未闭合。

这些测试只证明 Media contract 与局部 provider。完整 Headless Platform、统一测试 lifecycle、真实 PNG/WAV 和模型审查见 [Migration 11](../migrations/headless-platform-test-backend-migration.md)，当前状态为 `IN_PROGRESS`。

## Runtime 边界

Media Runtime 只消费 command，不写 VN route、backlog、read-state 或 save authority。视频、音频、滤镜等待点必须回到 AwaitToken/Fence，不能通过 provider callback 改 Runtime state。实现 trait、默认 provider 和 gate 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

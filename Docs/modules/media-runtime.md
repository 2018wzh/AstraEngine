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

- Renderer2D：真实 wgpu owner 位于 platform host 的 `WgpuPresentationCore`；Windows 路径覆盖 hardware surface、ordered frame、resize、context/device loss、retained frame resource rebuild 和 readback。文本 pass 通过 `surface.present_text_scene` 执行真实 GPU atlas、vertex/scissor draw 和事务性资源提交；非文本 `SceneCommand` 仍明确拒绝，不能把该 pass 外推成完整 GPU renderer。Headless capture 只作为 reference provider；Migration 11 planned 完整 CPU reference path。
- TextLayout：`astra.text_layout.v2` 的 cosmic-text/Swash provider；只装载 target/profile 允许且 hash/face/coverage 可验证的 package 字体，输出 shaped glyph、cluster/font identity 与 Alpha8/RGBA glyph resource。`TextRenderResourceOwner` 管理跨 frame upload/reference/release；`astra.text_layout_replay.v1` 固化 bounded provider/font/layout/glyph record，restore continuation 与 provider-free replay 都验证 package/build/session identity。Windows hardware glyph consumer 读取相同 command stream，并用字体 revision、layout hash、GPU capture hash 和变化像素形成 visual golden；失败 present 不提交逻辑资源，loss 后从 retained bitmap 重建。Player 不能自行估算字符矩形，也不能在 replay 时重新调用 live font provider。
- Decode：`DecodeBindingContext` 精确绑定 provider/target/profile；fallback 和 reference provider 都要显式授权。Windows WMF provider 已用 CC0 public MP3/MP4 fixture 验证 bounded PCM、BGRA 首帧、corrupt input diagnostic 和失败后 sequence retry。`ffmpeg-vcpkg` provider 已覆盖 native probe、真实 timestamped MP3/MP4 stream、目标格式 resample、seek generation、EOS drain、取消、packet backpressure、hash 和终段 trimming；`WindowsNativeMediaSession` 把同一 packet stream 接入 audio-master scheduler、WASAPI 和 wgpu。正式 release fallback 仍由 profile policy、native probe、same-run identity 和 reference performance pass共同决定，不能因 feature 或局部测试存在而自动启用。
- Audio：`astra.audio_graph.v2` 提供事务性 voice/fade 生命周期和 fixed-delta continuation；真实 Windows output 使用 bounded queue、WASAPI callback meter、错误传播和 close drain，不存在用 graph hash 冒充 meter 的产品 provider。 Migration 11 planned 完整 PCM WAV output。
- MediaPlayback：`astra.media_playback.v1` 以 audio callback playhead 为 A/V master clock，管理 timestamped audio/video packet、bounded queue、play/pause/seek/EOS/cancel、显式 late-video policy、事务 tick 和 validated snapshot restore。它是共享 scheduler，不替代真实 decode-to-output 接线。

## 测试

Media tests 覆盖 `astra.font_manifest.v1` 经 package VFS context 解析真实字体 section、packaged font hash/face/coverage、Latin/组合字符、ruby、BiDi paragraph、真实 glyph raster、wrap/clip/ellipsis、动态字体 cache invalidation、transactional renderer resource lifecycle、显式 decode binding、corrupt/truncated input、AudioGraph voice/fade/loop/seek/transaction、MediaPlayback audio-master clock/seek/EOS/drop policy/rollback/snapshot continuation、FilterGraph unknown/no-op/target bypass、Windows ordered frame/resize/readback、WASAPI queue/drain、WMF audio/video decode 和 optional FFmpeg 真实 timestamped decode。`text_surface` 覆盖 CJK/假名、Arabic、emoji 的 Windows hardware glyph golden，以及 retained texture/sprite/rect、duplicate upload rollback、resource release、loss/rebuild 和相同 capture hash；`windows_text_presentation` 与 `native_vn_host_source` 分别验证通用 packaged text 和 bundled VN localization/font/runtime command 经 `PlayerHostCommandExecutor`/`PlatformCommandSink` 到真实 GPU capture与 shutdown release。Web consumer、bundled VN camera/timeline/video/audio、GPU FilterGraph 与 reference performance pass 尚未闭合。

这些测试只证明 Media contract 与局部 provider。完整 Headless Platform、全 Runtime test 收束、真实 PNG/WAV 和模型审查见 [Migration 11](../migrations/headless-platform-test-backend-migration.md)，当前状态为 `SPEC_READY`。

## Runtime 边界

Media Runtime 只消费 command，不写 VN route、backlog、read-state 或 save authority。视频、音频、滤镜等待点必须回到 AwaitToken/Fence，不能通过 provider callback 改 Runtime state。实现 trait、默认 provider 和 gate 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

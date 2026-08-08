# Media Contract

Media 分为 Renderer2D、TextLayout、DecodeProvider、FilterGraph、Kira AudioService 和 Timeline。它们执行表现请求，不拥有剧情状态。

## Renderer2D

wgpu 是默认 provider，但不是唯一后端。真实 wgpu owner 是 platform host 的 `WgpuPresentationCore`；`astra-media` 不再用只返回 descriptor 的 facade 冒充 renderer。Windows host 持有 hardware adapter、device、surface、upload/readback resource 和 frame sequence，显式处理 resize、context loss、device loss、资源重建与失败事件。Headless CPU provider只用于 E1/E2 reference，不具备 shipping eligibility。

Migration 11 的 `CpuRendererProvider`、Kira mixer、Image/Symphonia decode 和 Headless artifact recorder 已接入统一 host，能够输出真实 PNG 与 PCM S16LE WAV。Headless 与 native host 共用 `AudioServiceSession` 和 `AudioOutputLane`，只替换最终 endpoint。完整产品证据仍须绑定 package、物理输入、provider、checkpoint 和 run identity；空音频、静态 meter 或 synthetic decode 均不计入验收。

## DecodeProvider

### RFVP PlatformHost stream binding

Windows RFVP movie playback uses the shared PlatformHost decode session rather
than a fixed-tick full-file decode. The lifecycle is
`OpenDecode -> Start -> Next* -> CloseDecode`. `Start` returns typed
`VideoStreamStart`; each `Next` returns one typed `VideoFrame` or PCM chunk, and
end-of-stream is reported as typed `VideoStreamEnd` or the stable
`ASTRA_MEDIA_STREAM_EOS` diagnostic.
The Windows provider requests Media Foundation hardware transforms. Its public
boundary is still CPU BGRA/i16 PCM, so the final CPU-to-WGPU/device transfer is
the only required output transfer and this contract does not claim external
texture zero-copy. `VideoFrame` 只携带 sequence、PTS、duration、尺寸和 owned
BGRA8 allocation；不携带 format string、postcard payload 或实时 content hash。
WMF host 将同一 allocation 直接移交给 consumer。Hardware-transform selection is requested at the WMF source-reader
boundary, but a device-specific transform-selection proof is still a separate
release gate.

Manager and native CLI use bounded prefetch workers and consume owned chunks at
the runtime boundary. The worker, scene present, and UI loops do not decode a
complete movie or append PCM by rebuilding historical samples. Web and other
platforms remain explicit provider-gated until their native stream contract is
implemented; no implicit software fallback is permitted.

Decode 只能通过 `DecodeBindingContext { provider_id, target, profile, allow_reference_provider }` 选择一个显式 provider。Registry 阻断重复 id、无 binding、profile drift、unsupported codec/kind、feature-gated provider 和 reference provider 进入 shipping；注册顺序不参与选择。平台 provider 包括 Windows Media Foundation；Image/Symphonia 只能由 profile 显式绑定，不能作为失败后的 fallback。`SyntheticPlatformDecodeProvider` 明确为 non-packaged reference provider。DecodeProvider 输出经过 provider/kind/codec 与 typed shape 校验的 CPU buffer、PCM、video stream item 或 `MediaSurfaceToken`，public API 不暴露平台 native handle。

桌面 FFmpeg 由 optional `ffmpeg-vcpkg` feature 声明，默认 build 不要求本机 FFmpeg。`astra-release`、`astra-cli` 和 Windows host 通过同名 feature 透传该能力；decode profile 只能显式绑定一个 provider，缺失或失败时直接阻断，不允许 `[wmf, ffmpeg]` 这类有序 fallback。feature 存在本身不构成 provider 可用证据。

`FfmpegPlaybackDecoder` 从受限临时输入执行真实 demux，输出带 generation、sequence、PTS、duration 和 resource id 的 typed PCM/BGRA packet。它支持设备目标格式 resample、seek、EOS flush、终段 trimming、取消和单 packet backpressure；`MediaPlaybackPipeline` 负责 live byte budget、audio-master 调度与视频资源释放，不在实时路径计算 packet hash。正式 performance evidence 仍绑定 product profile、package、build、session 与设备身份。

Headless 与 Windows 共享 typed incremental stream contract。`DecodeStreamAction::Start` 建立有界 session，后续 `Next` 每次消费一个 owned frame，EOF 返回 typed end marker。Player 同时最多保留一帧；snapshot 只保存 asset identity、revision、cursor、loop index 和逻辑起始时间。restore 重新创建 decode session并按 cursor continuation；skip、loop replacement、失败与 shutdown 必须显式 `CloseDecode`。

Player 从 package 消费 encoded audio 时，必须先通过 `asset.catalog` 与 `asset.vfs_manifest` 得到唯一 package-backed entry，执行 bounded read 和 SHA-256 校验，再按文件签名识别 codec。不能用 asset id、文件名或 provider descriptor 猜测已解码成功。Windows Media Foundation 当前返回 `pcm_s16le:<sample_rate>:<channels>`；Player 必须检查格式字段、采样率、声道、sample budget、sample 截断和 frame alignment，再显式转换为 interleaved `f32`。未知格式、空/越界 stream shape 和不完整 frame 都是 blocking，不能转为空音频成功。

Player 的音频路径固定为 `OpenDecode -> Decode -> CloseDecode -> AudioServiceSession -> AudioOutputLane -> CloseAudio`。实时 PCM 不进入 `HostCommand`，不计算 content hash，也不经过 `SubmitAudio`、`QueryAudio` 或 `DrainAudio`。Kira 根据设备消费的 sample 数生成 completion，Runtime 在下一个 fixed tick 接收 typed event。open 后任一步失败都必须关闭已创建资源；Kira 或 endpoint 提交失败会 poison 当前音频 session，不能生成替代完成事件。

持续音频由 session-local `AudioServiceSession` 管理。Kira 持有 voice、bus、fade、loop、seek、clock 与混音；平台只持有 `AudioOutputLane` endpoint。decoded audio 在播放前转换到设备格式并进入有界 cache，同格式 owned chunk 不重建。重复 voice、非法 gain/fade、容量、sequence、转换预算或 Kira 提交失败必须 poison 当前音频 session。`audio action:pause|resume|stop target:<stable-command-id>` 经过 ordered Runtime output 进入服务；missing target、重复 pause/resume 和未知 action 不得空成功。完成事件以设备已消费 sample 为准，在下一 fixed tick 回注 Runtime。

`AudioOutputLane` 暴露只读原子 telemetry 和 wake registration，Kira worker按队列容量补充 chunk；refill 不进入 window/present command FIFO。稳定泵送后 underflow 增长必须终止受影响 session。open 后若设备格式漂移必须 blocking，退出时停止 mixer、关闭 endpoint 并等待 worker join。Web 仍须由真实 keyboard/pointer user activation 触发 `AudioContext.resume()`；设备热切换恢复与正式浏览器 E3 evidence 仍是独立门禁。

PlatformHost 通过 `AudioOutputLane::submit` 消费 Kira 填满的 owned chunk，并归还一个耗尽 allocation 供下一次 render 复用。native callback 使用 chunk+offset 批量消费，不能逐 sample push/pop，也不能分配或解码。AstraEMU Manager、CLI、Headless 与 Windowed E2 共用同一 Kira worker；Runtime tick、GPU present 与 Slint event loop 只提交 typed command 和读取 telemetry。

RFVP 的 SubmitI16/SubmitF32 不再把样本放进 audio command postcard。Family ABI v7
直接移动 typed `I16`/`F32` packet，跨 ABI、Manager 和 worker 保留同一 ABI-owned
allocation。相同格式的 PCM 不允许重建；worker 只在实际 mix/resample 边界读取 chunk，
必要的采样格式转换单独记录。

Native callback 的消费、设备错误和低水位边沿通过 `AudioWakeRegistration` 唤醒 drain/refill waiter；waiter 使用绝对 deadline，不能以 4/5 ms `sleep` 或 fixed-tick timeout 反复查询。队列满时 producer 必须背压或返回稳定 overflow，设备丢失、worker panic 和 shutdown drain/abort/join 必须成为可诊断的终态。

## FilterGraph

视觉 FilterGraph 是 typed node graph：

```yaml
schema: astra.filter_graph.v1
targets: [background, character, ui, text, video, final]
nodes:
  - id: bloom_main
    kind: astra.filter.bloom
    input: final
    output: final
    params: { intensity: 0.35 }
```

Node 必须声明 input/output target、精确参数集合、GPU/CPU capability、determinism、fallback 和 release gate rule。当前 deterministic CPU executor 只接受已实现的 bloom、fade 和 color matrix，要求显式 `allow_cpu_fallback`，并阻断 unknown node、no-op fallback、跨 target 伪执行、参数缺失/多余/越界和损坏 frame。跨 target graph 与 GPU node provider 未闭合前保持未完成。

AstraEMU filter preset 复用同一 `FilterGraph`。final-frame preset 作用在合成后画面；per-layer preset 绑定 `PresentationCommand` 的 layer id 或 role。family 缺少 layer metadata 时，Manager 只启用 final-frame preset 并输出 diagnostic。

## Kira AudioService

Kira 是默认 mixer provider。`AudioTimelineStateV1` 只保存 bus、voice、fade、asset ID/URI/revision、sample cursor、loop 和 command sequence；Kira handle、平台 handle 与 PCM 不进入 save/replay。`ProductionAudioMixer`、旧 mixer snapshot 和实时 graph hash 已删除。非法 gain、资源 URI、状态迁移、冲突 fade、重复 id、容量和时间溢出必须在状态修改前失败。callback 报告 device loss 后，当前 session 必须停止提交并进入可诊断终态，不能静默切换 provider。

Headless reference output 使用固定采样率、固定声道布局的 PCM S16LE WAV，并保留完整 sample sequence。音频限额、写入失败、静音、削波、声道和时长不匹配都进入 machine-readable diagnostic；不能只记录 peak/RMS 后丢弃实际音频。

## MediaPlayback

`astra.media_playback.v1` 是 decode 与 platform output 之间的共享时序 owner。Session 显式声明 audio/video track、duration、queue/tick/audio-clock/video lead/lag budget 和 `LateVideoPolicy`；packet 绑定 generation、连续 sequence、PTS/duration、resource id 与 format/dimensions，不携带实时 content hash。audio callback playhead 是含音频 session 的 master clock。play/pause/seek/complete-seek/EOS/cancel/tick 使用单一状态机，seek 提升 generation 并清空旧资源；snapshot 保存 queue、EOS、clock、generation 和 sequence。

该 contract 只闭合共享 A/V scheduler、事务和 replay 边界。平台 decode provider 仍需产出真实 timestamped packet，Windows output 仍需把 scheduler output 接到 WGPU/WASAPI；没有这条产品接线时，不能把 `astra.media_playback.v1` 单独算作完整媒体 session 或 E3。

## TextLayout

默认 TextLayout provider 使用 `cosmic-text`/Swash，contract schema 为 `astra.text_layout.v2`。Provider 在创建时接收 `FontBindingContext { target, profile, default_locale }`、显式预算和 package 提供的字体集合；字体 descriptor 必须包含 asset id、family、face index、content hash、license、subset、Unicode coverage、target/profile eligibility 和实际 bytes。字体 hash、face metadata、eligibility 或 coverage 不一致时创建失败，不能转用系统字体。

`TextLayoutRequest` 明确声明 language、script、direction、OpenType feature、fallback family chain、wrap 和 overflow policy。输出是带 UTF-8 source cluster、实际 font face identity、glyph id、BiDi direction、advance、baseline、visual line、ruby placement 和 raster resource id 的 `ShapedGlyphRun`，不再输出按字符数估算的 box。`Clip` 和 ellipsis 是 contract 结果的一部分。普通 layout/measure/cache 使用 typed request key、字体 generation 和 provider-local layout revision，不执行 JSON/postcard 或 content hash；voice replay metadata 只在 replay/save 的 persisted record 中编码。

Swash raster 输出 `Alpha8` 或 `Rgba8` glyph bitmap。RGB subpixel mask 会先校验三通道长度，再折叠为与面板 stripe order 无关的 deterministic `Alpha8`；零尺寸且零 advance 的组合/连接 glyph 保留 shaping identity，但不创建伪 bitmap。`TextRenderResourceOwner` 负责跨 frame 引用计数、增量 upload、最后引用 release、重复 bitmap 冲突和 shutdown drain；command stream 失败时，headless renderer 不提交任何 resource mutation。缺字、未声明 fallback、fallback 顺序漂移、错误方向、字体 hash 漂移、无效 ruby range、资源冲突和预算超限分别返回稳定 `ASTRA_TEXT_*` diagnostic。

当前共享实现已经提供 packaged font database、`astra.font_manifest.v1` 到已验证 Package/VFS section 的权威读取、真实 shaping/raster、动态字体替换与 cache invalidation、cluster mapping、ruby、BiDi、wrap/clip/ellipsis 和 renderer-ready glyph command；它属于 E2 shared implementation。仓库内固定 revision/hash/OFL 的 Noto Sans SC、Noto Sans Arabic 和 Noto Emoji fixture 会作为真实 package sections 加载，覆盖 CJK/假名/ruby、Arabic RTL/组合字符、emoji variation/ZWJ cluster 与显式多字体 fallback。加密字体 section 必须显式提供匹配的 container crypto provider，未提供时直接失败。

`astra.text_layout_replay.v1` 和 `astra.text_layout_replay_snapshot.v1` 把 package、build、session、provider fingerprint、target/profile、完整 font identity、request identity、layout revision 和 renderer-ready glyph payload 固化为 bounded binary transcript。request、record、transcript 和 snapshot digest 只在 replay/save 边界生成，不进入普通 layout 或 render 帧预算。Live continuation 会先验证实际 provider/font identity，再事务追加 record；provider-free replay 不加载字体 provider，只按顺序消费已校验 record。request、provider、font、package、bitmap、sequence、record/transcript hash 或容量发生漂移都会以稳定 `ASTRA_TEXT_REPLAY_*`/`ASTRA_TEXT_PROVIDER_DRIFT` 错误阻断，失败不会推进 cursor 或修改 transcript。

Windows host 的 `surface.present_text_scene` 直接消费上述 glyph command。`WgpuGlyphAtlasRenderer` 校验 typed glyph identity、资源 ID、引用和 clip 栈，在 64 MiB/65,536 resource 上限内构建硬件 atlas、vertex batch 和 scissor draw；逻辑资源只在 surface present 成功后提交。`capture_surface` 读取同一次 GPU render 的 offscreen texture，`astra.windows_gpu_glyph_golden.v1` 的 layout/capture digest 由提交后的 Evidence consumer 生成。设备恢复会重建 pipeline、atlas 和 retained frame；test driver 只模拟 loss 后的 retained-resource rebuild，不冒充物理 GPU 移除。重复 upload、未知 release、非文本 command、尺寸错误和失败后的 sequence retry均为 blocking。

`PlayerHostCommand::PresentScene` 只承载 renderer-ready command，`PlatformCommandSink` 不生成 CPU frame。Windows retained atlas执行 `UploadGlyph`、`UploadTexture`、`ReleaseResource`、`GlyphRun`、`Sprite`、`Rect` 与 clip，并在非法 resource、重复 upload、越界 source、失败 present 和 device loss 时保持事务边界。`astra.player_presentation_report.v1` 从同 run hardware capture 记录 package/profile/build/session、renderer/font provider、layout/command/capture hash 和变化像素；release consumer要求它与 capability、conformance、automation identity 连续，headless、空画面和 drift 都会阻断。bundled VN 的 dialogue/choice/system text 已从 verified package font/localization 进入同一 Windows GPU stream；camera、timeline、video、audio 与 WebGPU 仍会显式阻断，因此 P1-001 不能标记 `RESOLVED`。

## Command Boundary

Runtime 只发 `PresentationCommand`、`AudioCommand`、`TimelineCommand` 和 graph refs。Media provider 只能回传 capability、AwaitResult、diagnostic、capture hash 和 profiling evidence。具体 trait、默认 provider 和 graph validation 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

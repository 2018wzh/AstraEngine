# Media Contract

Media 分为 Renderer2D、TextLayout、DecodeProvider、FilterGraph、AudioGraph 和 Timeline。它们执行表现请求，不拥有剧情状态。

## Renderer2D

wgpu 是默认 provider，但不是唯一后端。真实 wgpu owner 是 platform host 的 `WgpuPresentationCore`；`astra-media` 不再用只返回 descriptor 的 facade 冒充 renderer。Windows host 持有 hardware adapter、device、surface、upload/readback resource 和 frame sequence，显式处理 resize、context loss、device loss、资源重建与失败事件。Headless CPU provider只用于 E1/E2 reference，不具备 shipping eligibility。

现有 `HeadlessRenderer` 只证明轻量 CPU contract 和 deterministic frame。Migration 11 planned 完整 Headless Platform 必须显式绑定 Media 层的真实 renderer、font/TextLayout、FilterGraph、AudioGraph 和 decode provider，输出真实 PNG/WAV。state hash 颜色块、矩形变化、空音频、静态 meter 或 synthetic decode 都不能作为完整 Headless 产品证据。

## DecodeProvider

Decode 只能通过 `DecodeBindingContext { provider_id, target, profile, allow_fallback, allow_reference_provider }` 选择一个显式 provider。Registry 阻断重复 id、无 binding、profile drift、unsupported codec/kind、feature-gated provider、未声明 fallback 和 reference provider 进入 shipping；注册顺序不参与选择。平台 provider 包括 Windows Media Foundation；CPU fallback 包括 Image/Symphonia。`SyntheticPlatformDecodeProvider` 明确为 non-packaged reference provider。DecodeProvider 输出经过 provider/kind/codec/hash 校验的 CPU buffer 或 `MediaSurfaceToken`，public API 不暴露平台 native handle。

桌面 FFmpeg 由 optional `ffmpeg-vcpkg` feature 声明，默认 build 不要求本机 FFmpeg。`astra-release`、`astra-cli` 和 Windows host 通过同名 feature 透传该能力；启用 FFmpeg 的 Windows profile 必须按顺序声明 `decode.providers: [wmf, ffmpeg]` 并设置 `allow_software: true`，同时证明 native probe 通过。其他 provider 顺序、未声明软件 fallback 或只存在 feature 都会阻断。

`FfmpegPlaybackDecoder` 从受限临时输入执行真实 demux，输出带 generation、sequence、PTS、duration、resource id 和 content hash 的 PCM/BGRA packet。它支持设备目标格式 resample、seek、EOS flush、终段 trimming、取消和单 packet backpressure；`MediaPlaybackPipeline` 负责 payload hash/size、live byte budget、audio-master 调度与视频资源释放。Windows 的 `WindowsNativeMediaSession` 把同一 stream 接到 WASAPI 和 wgpu surface，覆盖 pause/resume、seek、显式 late-frame policy、设备丢失后的同格式重建以及失败清理。Session 现在还强制接收 product-profile-bound performance budget 与 source/package/build/session identity，并输出真实 measured report；host profile hash 绑定平台配置，product profile 绑定 package，二者不能混用。普通 debug test 可能按阈值诚实返回 blocked，不能把 report 存在当成性能通过。完整规则见 [Performance Contract](performance.md)。Release validator 已能校验 budget/report/capability/conformance/Player 的同 run identity，但当前尚无正式 reference environment 的 pass artifact，因此不能单独关闭 Windows Player E3 或完整 release fallback gate。

Player 从 package 消费 encoded audio 时，必须先通过 `asset.catalog` 与 `asset.vfs_manifest` 得到唯一 package-backed entry，执行 bounded read 和 SHA-256 校验，再按文件签名识别 codec。不能用 asset id、文件名或 provider descriptor 猜测已解码成功。Windows Media Foundation 当前返回 `pcm_s16le:<sample_rate>:<channels>`；Player 必须检查格式字段、采样率、声道、sample budget、sample 截断和 frame alignment，再显式转换为 interleaved `f32`。未知格式、空/越界 stream shape 和不完整 frame 都是 blocking，不能转为空音频成功。

Player 的一次性音频必须执行完整资源事务：`OpenDecode -> Decode -> CloseDecode -> OpenAudio -> ordered SubmitAudio -> DrainAudio -> CloseAudio`。每一步都要校验返回的 logical resource；decoded bytes 必须重新计算 hash；drain 返回的 sample count 必须与提交量一致，meter 必须是有限且不高于 `0 dBFS` 的同 run 结果。open 后任一步失败都必须尝试 close，并把 cleanup failure 附加到原始 diagnostic，不能用 `Unit`、空 meter 或静态 report 冒充播放成功。`voice_end` 只能由真实 sample mixer 报告 voice completion 后产生。

持续音频由 Player 持久 mixer owner 管理。平台先通过 `audio.format` 返回实际 preferred sample rate/channel；decoded audio 使用 `rubato` bounded sinc converter 对采样率做带限转换，只在同布局、mono→stereo 或 stereo→mono 的语义可证明时映射声道，缺 channel layout metadata 的 surround 映射必须 blocking。mixer 使用固定输出格式、显式 bus、bounded voice/render budget、逐 frame fade、loop cursor、sample clamp 和稳定 completion；重复 voice、非法 gain/fade、voice/render/converted-sample 超限必须 blocking。`audio action:pause|resume|stop target:<stable-command-id>` 经过 ordered Runtime output 进入 owner；missing target、重复 pause/resume 和未知 action 不得空成功。控制在已提交的 bounded queue 边界生效。

平台 `audio.query` 返回 queued/consumed/submitted frame、meter 和 callback underflow，Player 只补到目标 queue 水位；启动期建立 underflow baseline，稳定泵送后 underflow 增长必须以 `ASTRA_PLAYER_AUDIO_UNDERFLOW` 终止受影响 session。Windows/WASAPI 和 Web/AudioWorklet 使用同一 queue-state contract，open 后若设备格式相对协商结果发生变化必须 blocking；drain deadline 按实际提交时长加 callback margin 计算，不能使用固定两秒或十秒超时。退出时必须 drain 并 close。Web 必须由真实 keyboard/pointer user activation 触发 `AudioContext.resume()`，不得在 page load 时伪造 gesture；随后由共享 `NativeVnProductMediaHost` 统一执行 timeline、decode、mixer、wait completion 和 cleanup，其内部音频 owner 为 `NativeVnProductAudioHost`。设备热切换恢复、正式浏览器 E3 evidence 仍是独立未完成门禁，不能由 native `web-code-check` 或 mixer unit test 替代。

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

## AudioGraph

`astra.audio_graph.v2` 独立于视觉 FilterGraph。共享 owner 提供 bounded bus/voice/fade/fence、显式 voice id、play/pause/resume/seek/stop、loop position、fixed-delta tick、事务性 command 和 deterministic snapshot hash；非法 gain、资源 URI、状态迁移、冲突 fade、重复 id、容量和时间溢出都必须在修改状态前失败。真实输出由 platform host 的 bounded audio queue 和 WASAPI callback 持有，close/shutdown 必须 drain；callback 报告 device loss 后 submit/drain 必须立即失败、销毁失效资源并发出 typed `DeviceLost` event，不能继续接受音频。graph hash 不是音频 meter，也不能替代真实 callback meter、A/V sync 或听测证据。

Headless reference output 使用固定采样率、固定声道布局的 PCM S16LE WAV，并保留完整 sample sequence。音频限额、写入失败、静音、削波、声道和时长不匹配都进入 machine-readable diagnostic；不能只记录 peak/RMS 后丢弃实际音频。

## MediaPlayback

`astra.media_playback.v1` 是 decode 与 platform output 之间的共享时序 owner。Session 显式声明 audio/video track、duration、queue/tick/audio-clock/video lead/lag budget 和 `LateVideoPolicy`；packet 必须绑定 generation、连续 sequence、PTS/duration、resource id、format/dimensions 与 content hash。audio callback playhead 是含音频 session 的 master clock，缺失、回退、跳变、超时长和未声明 drop 都 blocking。play/pause/seek/complete-seek/EOS/cancel/tick 都有稳定状态机，seek 提升 generation 并清空旧资源，非法 tick 和 A/V drift 在提交前失败。snapshot 保存完整 queue、EOS、clock、generation 和 sequence；restore 会重新验证 schema、预算、时序和资源 identity，恢复后的 continuation hash 必须与 uninterrupted run 一致。

该 contract 只闭合共享 A/V scheduler、事务和 replay 边界。平台 decode provider 仍需产出真实 timestamped packet，Windows output 仍需把 scheduler output 接到 WGPU/WASAPI；没有这条产品接线时，不能把 `astra.media_playback.v1` 单独算作完整媒体 session 或 E3。

## TextLayout

默认 TextLayout provider 使用 `cosmic-text`/Swash，contract schema 为 `astra.text_layout.v2`。Provider 在创建时接收 `FontBindingContext { target, profile, default_locale }`、显式预算和 package 提供的字体集合；字体 descriptor 必须包含 asset id、family、face index、content hash、license、subset、Unicode coverage、target/profile eligibility 和实际 bytes。字体 hash、face metadata、eligibility 或 coverage 不一致时创建失败，不能转用系统字体。

`TextLayoutRequest` 明确声明 language、script、direction、OpenType feature、fallback family chain、wrap 和 overflow policy。输出是带 UTF-8 source cluster、实际 font face/hash、glyph id、BiDi direction、advance、baseline、visual line、ruby placement 和 raster resource id 的 `ShapedGlyphRun`，不再输出按字符数估算的 box。`Clip` 和 ellipsis 是 contract 结果的一部分；voice replay metadata 与 layout identity 一起参与 hash。

Swash raster 输出 `Alpha8` 或 `Rgba8` glyph bitmap。RGB subpixel mask 会先校验三通道长度，再折叠为与面板 stripe order 无关的 deterministic `Alpha8`；零尺寸且零 advance 的组合/连接 glyph 保留 shaping identity，但不创建伪 bitmap。`TextRenderResourceOwner` 负责跨 frame 引用计数、增量 upload、最后引用 release、重复 bitmap 冲突和 shutdown drain；command stream 失败时，headless renderer 不提交任何 resource mutation。缺字、未声明 fallback、fallback 顺序漂移、错误方向、字体 hash 漂移、无效 ruby range、资源冲突和预算超限分别返回稳定 `ASTRA_TEXT_*` diagnostic。

当前共享实现已经提供 packaged font database、`astra.font_manifest.v1` 到已验证 Package/VFS section 的权威读取、真实 shaping/raster、动态字体替换与 cache invalidation、cluster mapping、ruby、BiDi、wrap/clip/ellipsis 和 renderer-ready glyph command；它属于 E2 shared implementation。仓库内固定 revision/hash/OFL 的 Noto Sans SC、Noto Sans Arabic 和 Noto Emoji fixture 会作为真实 package sections 加载，覆盖 CJK/假名/ruby、Arabic RTL/组合字符、emoji variation/ZWJ cluster 与显式多字体 fallback。加密字体 section 必须显式提供匹配的 container crypto provider，未提供时直接失败。

`astra.text_layout_replay.v1` 和 `astra.text_layout_replay_snapshot.v1` 把 package、build、session、provider fingerprint、target/profile、完整 font identity、request hash、layout hash 和 renderer-ready glyph payload 固化为 bounded binary transcript。Live continuation 会先验证实际 provider/font identity，再事务追加 record；provider-free replay 不加载字体 provider，只按顺序消费已校验 record。request、provider、font、package、bitmap、sequence、record/transcript hash 或容量发生漂移都会以稳定 `ASTRA_TEXT_REPLAY_*`/`ASTRA_TEXT_PROVIDER_DRIFT` 错误阻断，失败不会推进 cursor 或修改 transcript。P1-001 仍等待 Windows GPU glyph atlas visual golden，以及把该 transcript 接入正式产品 presentation/release evidence；不能据此宣称字体产品能力达到 E3。

## Command Boundary

Runtime 只发 `PresentationCommand`、`AudioCommand`、`TimelineCommand` 和 graph refs。Media provider 只能回传 capability、AwaitResult、diagnostic、capture hash 和 profiling evidence。具体 trait、默认 provider 和 graph validation 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。

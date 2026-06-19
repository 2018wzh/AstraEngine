# Media Backend Production Contract

状态：Production contract draft / not yet fully implemented  
定位：定义 Renderer2D、TextLayout、Audio、Timeline 和 executable FilterGraph 的 provider 执行契约。解码能力由独立 Decode Provider 提供，见 [Hardware Media Decode](hardware-media-decode.md)。

## 1. 目标

- PresentationCommand 不直接调用 renderer/audio/font native API，而是进入 RenderGraph、TextLayoutRequest、AudioCommand 和 Timeline state。
- Renderer2D/TextLayout/Audio provider 可通过 EngineModuleSlot 替换。
- Headless backend 与真实 backend 共享 logical graph 和 deterministic hash。
- Provider 不向 public ABI 暴露 SDL、D3D、Vulkan、Metal、audio device、font rasterizer private object 或 Editor widget。

非目标：

- 不实现复杂 3D renderer、physics renderer、大型开放世界 streaming renderer。
- 不让 Media provider 拥有 Runtime 主循环。

## 2. Provider Slots

Production slots：

```yaml
engine_modules:
  selections:
    astra.renderer2d: astra.renderer2d.default
    astra.text_layout: astra.text_layout.default
    astra.audio: astra.audio.default
    astra.timeline: astra.timeline.default
    astra.filter_graph: astra.filter_graph.default
```

Provider descriptor 公共字段：

```yaml
provider_id: astra.renderer2d.bgfx
contract: IRenderer2DProvider
slot_id: astra.renderer2d
features:
  - texture_import
  - render_target
  - frame_capture
  - filter_graph_execution
headless_fallback: astra.renderer2d.headless
packaged_eligible: true
diagnostics_prefix: ASTRA_RENDERER2D
```

## 3. Renderer2D Contract

准接口：

```cpp
class IRenderer2DProvider {
public:
    virtual Renderer2DProviderDescriptor Describe() const = 0;
    virtual Result<void> BeginFrame(const RenderFrameDesc&, DiagnosticSink&) = 0;
    virtual Result<TextureToken> ImportTexture(const DecodedCpuBuffer&, DiagnosticSink&) = 0;
    virtual Result<TextureToken> ImportSurface(MediaSurfaceToken, DiagnosticSink&) = 0;
    virtual Result<void> Execute(const RenderGraph&, DiagnosticSink&) = 0;
    virtual Result<FrameCapture> Capture(FrameCaptureRequest, DiagnosticSink&) = 0;
    virtual Result<void> Present(PresentRequest, DiagnosticSink&) = 0;
};
```

Rules:

- `TextureToken` is provider-scoped, transient and non-serializable.
- Zero-copy import is optional; provider must report fallback from `MediaSurfaceToken` to CPU upload.
- Device lost triggers recreate; failed recreate emits fatal runtime diagnostic with crash bundle context.
- Render target ids are logical DTO ids; native render target handles stay private.

## 4. TextLayout Contract

准接口：

```cpp
class ITextLayoutProvider {
public:
    virtual TextLayoutProviderDescriptor Describe() const = 0;
    virtual Result<GlyphRun> Shape(TextLayoutRequest, DiagnosticSink&) = 0;
    virtual Result<GlyphAtlasToken> PrepareAtlas(GlyphRun, DiagnosticSink&) = 0;
    virtual Result<TextLayoutCapture> Capture(TextCaptureRequest, DiagnosticSink&) = 0;
};
```

Rules:

- Save stores text key, style, typewriter progress and locale, not glyph atlas token.
- Missing glyph diagnostics include locale, font asset id and codepoint.
- Headless path emits glyph run hash.

## 5. Audio Contract

准接口：

```cpp
class IAudioProvider {
public:
    virtual AudioProviderDescriptor Describe() const = 0;
    virtual Result<AudioStreamToken> CreateStream(AudioCommand, DiagnosticSink&) = 0;
    virtual Result<void> Submit(AudioGraph, DiagnosticSink&) = 0;
    virtual Result<AudioStateCapture> Capture(AudioCaptureRequest, DiagnosticSink&) = 0;
};
```

Rules:

- Audio provider may consume decoded PCM or provider-owned stream tokens once the future decode provider contract is implemented; current Phase 7 evidence records audio metadata and logical bus state.
- Save stores logical bus state, currently playing logical cues, timeline sync and fade state.
- Device unavailable falls back to silent provider when release profile allows it.

## 6. Timeline And FilterGraph

Timeline provider:

```yaml
schema: astra.media.timeline_state.v1
timeline_id: native:/Timelines/Opening
cursor_time_ns: 1200000000
active_tracks: []
pending_events: []
```

FilterGraph execution:

- Filter pass target must be one of `background`, `character`, `ui`, `text`, `final`.
- GPU FilterGraph provider may execute shader passes; headless provider records pass hash and target hash.
- Unsupported filter in deterministic release is blocking unless profile allows configured fallback.

## 7. Diagnostics And Acceptance

Diagnostic prefixes:

- `ASTRA_RENDERER2D_*`
- `ASTRA_TEXT_LAYOUT_*`
- `ASTRA_AUDIO_*`
- `ASTRA_TIMELINE_*`
- `ASTRA_FILTER_GRAPH_*`

`MediaBackend` sample must prove:

- selected Decode/Renderer/Text/Audio providers pass release gate.
- package payload image/font/audio decode flows into real provider execution.
- headless and real backend produce comparable logical hashes.
- GPU filter fallback and unsupported format diagnostics are covered.



# Hardware Media Decode Contract

状态：Production contract draft / not yet implemented  
定位：定义独立 Image/Audio/Video Decode Provider。硬件加速解码不并入 Renderer2D 或 AudioProvider；Renderer2D/Audio 只消费解码输出或 provider-owned opaque token。

## 1. 目标

- 支持 CPU decode、hardware decode 和 zero-copy GPU surface import 的能力协商。
- Image、Audio、Video decode provider 可独立替换、测试和 release gate。
- 不暴露 D3D、Vulkan、Metal、VAAPI、VideoToolbox、Media Foundation、NVDEC、AMF、QSV、SDL 或 OS handle。
- Runtime save/replay 保存 logical asset and timing state，不保存 decoder native state。

非目标：

- 硬件解码不是 Core 或 Platform public API。
- 不要求所有平台都有硬解；release profile 决定 fallback 是否可接受。

## 2. Slots And Descriptors

Slots:

```yaml
engine_modules:
  selections:
    astra.image_decode: astra.decode.image.foundation
    astra.audio_decode: astra.decode.audio.foundation
    astra.video_decode: astra.decode.video.platform
```

Decode provider descriptor:

```yaml
schema: astra.media.decode_provider.v1
provider_id: astra.decode.video.media_foundation
contract: IVideoDecodeProvider
slot_id: astra.video_decode
hardware_accelerated: true
zero_copy_supported: true
headless_supported: false
packaged_eligible: true
diagnostics_prefix: ASTRA_DECODE_VIDEO_MF
capabilities: []
```

Capability:

```yaml
schema: astra.media.decode_capability.v1
media_kind: video
container: mp4
codec: h264
profile: high
level: "4.1"
bit_depth: 8
pixel_formats: [nv12, rgba8]
max_resolution: [3840, 2160]
hdr: false
alpha: false
hardware_paths: [d3d11, d3d12]
cpu_fallback: true
zero_copy_targets: [astra.renderer2d]
```

## 3. Requests And Outputs

Decode request:

```yaml
schema: astra.media.decode_request.v1
asset_id: native:/Movies/Opening
payload_ref: package:/Movies/Opening.mp4
media_kind: video
preferred_hardware: true
allow_cpu_fallback: true
target_provider: astra.renderer2d.sdl_gpu
frame_time_ns: 1200000000
```

CPU image output:

```yaml
schema: astra.media.decoded_cpu_buffer.v1
format: rgba8
width: 1920
height: 1080
row_stride: 7680
color_space: srgb
buffer_id: transient:/decode/frame120
```

Audio output:

```yaml
schema: astra.media.decoded_audio_pcm.v1
sample_format: f32
channels: 2
sample_rate: 48000
frame_count: 2048
channel_layout: stereo
```

Video frame output:

```yaml
schema: astra.media.decoded_video_frame.v1
frame_index: 120
presentation_time_ns: 1200000000
output_kind: media_surface
surface_token: surface:/provider-local/42
fallback_used: false
```

`MediaSurfaceToken` rules:

- provider-scoped opaque token。
- not serializable。
- only valid until declared lifetime fence。
- may be imported only by compatible Renderer2D provider or converted to CPU buffer by decoder.

## 4. Interfaces

准接口：

```cpp
class IImageDecodeProvider {
public:
    virtual DecodeProviderDescriptor Describe() const = 0;
    virtual Result<ImageDecodeMetadata> Inspect(DecodeRequest, DiagnosticSink&) = 0;
    virtual Result<DecodedCpuBuffer> DecodeImage(DecodeRequest, DiagnosticSink&) = 0;
};

class IAudioDecodeProvider {
public:
    virtual DecodeProviderDescriptor Describe() const = 0;
    virtual Result<AudioDecodeMetadata> Inspect(DecodeRequest, DiagnosticSink&) = 0;
    virtual Result<DecodedAudioPcm> DecodeAudio(DecodeRequest, DiagnosticSink&) = 0;
};

class IVideoDecodeProvider {
public:
    virtual DecodeProviderDescriptor Describe() const = 0;
    virtual Result<VideoDecodeMetadata> Open(DecodeRequest, DiagnosticSink&) = 0;
    virtual Result<DecodedVideoFrame> DecodeFrame(VideoDecodeFrameRequest, DiagnosticSink&) = 0;
    virtual Result<void> Flush(DecodeStreamToken, DiagnosticSink&) = 0;
};
```

## 5. Fallback And Release Gate

Fallback policy:

```yaml
schema: astra.media.decode_fallback_policy.v1
allow_cpu_fallback: true
allow_silent_audio: false
allow_placeholder_frame: false
require_hardware_for_profiles:
  - codec: hevc
    resolution_min: [3840, 2160]
```

Release Gate blocks:

- selected decode provider missing or not packaged eligible.
- asset codec/profile unsupported by selected provider and fallback policy.
- zero-copy required but target renderer cannot import provider surface.
- deterministic profile depends on platform codec behavior without headless verification path.

Diagnostics:

- `ASTRA_DECODE_PROVIDER_MISSING`
- `ASTRA_DECODE_CODEC_UNSUPPORTED`
- `ASTRA_DECODE_HARDWARE_UNAVAILABLE`
- `ASTRA_DECODE_SURFACE_IMPORT_FAILED`
- `ASTRA_DECODE_FALLBACK_USED`
- `ASTRA_DECODE_STREAM_CORRUPT`

## 6. Acceptance

`MediaBackend` must cover:

- CPU image decode path.
- hardware-capability report path even when hardware is unavailable.
- video provider fallback to CPU or placeholder according to profile.
- zero-copy compatibility failure diagnostic.
- packaged payload decode using PackageReader, not source path.


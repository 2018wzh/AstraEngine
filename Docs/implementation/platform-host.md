# Platform Host Blueprint

平台模块只适配原生能力，不拥有引擎状态。六个 v1 目标平台都必须输出 capability report 并通过 profile gate。当前 Windows 和 Web 的真实 smoke 已落地；Linux、macOS、iOS 和 Android host completion 移到 Stage 6。

Target 与 Platform 的共同规则见 [Target And Platform Blueprint](target-platform.md)。本页只展开 host adapter。

## Host Trait

```rust
pub trait PlatformHost {
    fn descriptor(&self) -> PlatformDescriptor;
    fn create_surface(&mut self, request: SurfaceRequest) -> PlatformResult<SurfaceToken>;
    fn poll_input(&mut self) -> PlatformResult<Vec<PlayerInput>>;
    fn audio_output(&mut self) -> PlatformResult<AudioOutputToken>;
    fn decode_provider(&mut self) -> PlatformResult<ProviderId>;
    fn save_store(&mut self) -> PlatformResult<SaveStoreToken>;
    fn capability_report(&self) -> PlatformCapabilityReport;
}
```

`SurfaceToken`、`AudioOutputToken`、`SaveStoreToken` 是平台私有 token。Runtime public API 不暴露 native handle。

## Crate Split

| Crate | 职责 |
| --- | --- |
| `astra-platform` | `PlatformId`、`SdkStatus`、`PlatformCapabilityReport`、`PlatformHost` trait |
| `astra-platform-windows` | Windows host adapter、hidden window、wgpu surface、WMF audio/video decode、WASAPI、DPI/IME capability |
| `astra-platform-linux` | Linux capability crate；window system/audio/font/decode smoke 待实现 |
| `astra-platform-macos` | macOS capability crate；AppKit/AVFoundation/CoreAudio smoke 待实现 |
| `astra-platform-ios` | iOS capability crate；launcher、safe area、touch、AVFoundation、no-JIT Luau gate 待实现 |
| `astra-platform-android` | Android capability crate；launcher、MediaCodec、SAF、audio focus、no-JIT Luau gate 待实现 |
| `astra-platform-web` | Web host probe；WASM browser、renderer context、browser media decode、WebCodecs config、WebAudio render、OPFS/IndexedDB、File API/fetch package source smoke |

每个 host crate 可以用平台私有类型实现内部 bridge，但 public report 和 Runtime 入口只传 DTO 或 token。

## Platform Matrix

| Platform | Shell | Render | Decode | Save | Required Gate |
| --- | --- | --- | --- | --- | --- |
| Windows | winit | wgpu | WMF, FFmpeg fallback | user data dir | desktop-release |
| Linux | winit | wgpu | GStreamer/FFmpeg profile | XDG data | desktop-release |
| macOS | winit/AppKit bridge | wgpu | AVFoundation | app support | desktop-release |
| iOS | Swift/SwiftUI launcher | wgpu/Metal | AVFoundation | app container | mobile-release |
| Android | Kotlin launcher | wgpu/Vulkan | MediaCodec | app storage + SAF import | mobile-release |
| Web | WASM host | WebGPU/WebGL profile | WebCodecs | OPFS/IndexedDB | web-release |

## Capability Report

```yaml
schema: astra.platform_capability_report.v1
platform: windows
renderer:
  backend: wgpu
  headless: true
decode:
  video: [wmf, ffmpeg]
audio:
  output: wasapi
filesystem:
  package_sources: [file, http_range]
permissions:
  network_runtime_ai: profile_gated
smoke:
  - id: windowed_smoke
    status: pass
    summary: winit hidden window created with active event loop and IME cursor area
    evidence:
      - key: width
        value: "320"
      - key: height
        value: "180"
  - id: renderer.wgpu_surface
    status: pass
    summary: wgpu surface and compatible adapter were created for the hidden window
    evidence:
      - key: format_count
        value: "8"
  - id: decode.wmf.video_first_frame
    status: pass
    summary: WMF decoded the public MP4 fixture into a CPU first frame
    evidence:
      - key: format
        value: bgra8:first_frame:960x540
      - key: hash
        value: sha256:...
```

Required smoke 按平台分开定义。Windows 当前要求 `windowed_smoke`、`renderer.wgpu_surface`、`decode.wmf.audio`、`decode.wmf.video_first_frame`、`audio.wasapi` 和 `save.known_folder_rw`。Web 当前要求 `browser_smoke`、`renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw` 和 `package.web_source_read`。Linux 计划要求 `windowed_smoke` 和 `decode.linux_media`；macOS 计划要求 `windowed_smoke` 和 `decode.avfoundation`；iOS 计划要求 `launcher_smoke` 和 `decode.avfoundation`；Android 计划要求 `launcher_smoke` 和 `decode.mediacodec`。

## Checks

```bash
astra platform probe --platform windows --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release
astra test run scenarios/platform_smoke.yaml --headless --report target/platform-smoke.yaml
```

Expected report: launch、resize、input、audio、decode、save persistence、package import、provider-free replay 按平台 profile 通过。

普通 CI 只要求 schema、Target validation 和 capability report 可生成。真实平台完成需要 `sdk_status: present` 加对应平台 required smoke 通过；`sdk_status: missing` 或缺 required smoke 会阻断该平台的 DONE 证据。

# Platform Host Blueprint

平台模块只适配原生能力，不拥有引擎状态。六个 v1 目标平台都必须输出 capability report 并通过 profile gate。

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
```

## Checks

```bash
astra platform probe --platform windows --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release
astra test run scenarios/platform_smoke.yaml --headless --report target/platform-smoke.yaml
```

Expected report: launch、resize、input、audio、decode、save persistence、package import、provider-free replay 按平台 profile 通过。

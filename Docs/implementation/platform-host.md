# Platform Host Blueprint

平台模块只适配原生能力，不拥有引擎状态。六个 v1 目标平台都必须输出 capability report 并通过 profile gate。当前只有 Windows 的真实 smoke 已落地；其他平台的 host smoke 是 Stage 2 后续缺口。

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
| `astra-platform-windows` | Windows host adapter、windowed smoke、WMF/WASAPI/DPI/IME capability |
| `astra-platform-linux` | Linux capability crate；window system/audio/font/decode smoke 待实现 |
| `astra-platform-macos` | macOS capability crate；AppKit/AVFoundation/CoreAudio smoke 待实现 |
| `astra-platform-ios` | iOS capability crate；launcher、safe area、touch、AVFoundation、no-JIT Luau gate 待实现 |
| `astra-platform-android` | Android capability crate；launcher、MediaCodec、SAF、audio focus、no-JIT Luau gate 待实现 |
| `astra-platform-web` | Web capability crate；WASM host、WebGPU/WebGL、WebCodecs、WebAudio、OPFS/IndexedDB smoke 待实现 |

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
    summary: winit hidden window created; dpi and ime checked
  - id: decode.wmf
    status: pass
    summary: Media Foundation startup/shutdown completed
```

Required smoke 按平台分开定义。Windows 当前要求 `windowed_smoke`、`decode.wmf` 和 `save.known_folder`。Linux 计划要求 `windowed_smoke`、`decode.linux_media`；macOS 计划要求 `windowed_smoke`、`decode.avfoundation`；iOS 计划要求 `launcher_smoke`、`decode.avfoundation`；Android 计划要求 `launcher_smoke`、`decode.mediacodec`；Web 计划要求 `browser_smoke`、`decode.webcodecs`。

## Checks

```bash
astra platform probe --platform windows --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release
astra test run scenarios/platform_smoke.yaml --headless --report target/platform-smoke.yaml
```

Expected report: launch、resize、input、audio、decode、save persistence、package import、provider-free replay 按平台 profile 通过。

普通 CI 只要求 schema、Target validation 和 capability report 可生成。真实平台完成需要 `sdk_status: present` 加对应平台 required smoke 通过；`sdk_status: missing` 或缺 required smoke 会阻断该平台的 DONE 证据。

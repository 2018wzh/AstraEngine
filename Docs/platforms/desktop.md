# Desktop Platforms

Desktop host 覆盖 Windows、Linux、macOS。窗口和输入默认使用 winit，渲染默认 wgpu，解码优先平台 API，FFmpeg/vcpkg 作为 fallback。

对应 crate 是 `astra-platform-windows`、`astra-platform-linux` 和 `astra-platform-macos`。每个平台先输出 `astra.platform_capability_report.v1`，再进入 windowed smoke 和 release profile gate。当前 Stage 2 只把 Windows 作为 desktop host 完成；Linux 和 macOS 移到 Stage 6，仍是 `SPEC_READY`。

## Responsibilities

- 创建 surface、处理 DPI、窗口、输入法、手柄、文件选择、权限和 crash bundle。
- 提供 platform decode provider、filesystem provider、secret provider 和 system integration。
- 不保存 Runtime state，不直接调用 Actor 或 StateMachine 内部结构。

## Release Gate

每个平台必须跑 package launch、headless scenario、windowed smoke、save/load/replay、audio output probe、decode fallback 和 plugin fingerprint check。

## Current Status

| Work ID | Platform | Status | Scope |
| --- | --- | --- | --- |
| `S2-WINDOWS-HOST-01` | Windows | `DONE` | winit hidden window、`renderer.wgpu_surface`、DPI、IME、input event loop、XInput、`audio.wasapi`、`save.known_folder_rw` 和 SDK 状态已进入 capability report |
| `S2-WINDOWS-WMF-01` | Windows | `DONE` | WMF `DecodeProvider` 对 CC0 public fixture 执行 `decode.wmf.audio` MP3 PCM 和 `decode.wmf.video_first_frame` MP4 BGRA 首帧；失败返回 blocking diagnostic |
| `S2-WINDOWS-GATE-01` | Windows | `DONE` | Release Gate 要求 `windowed_smoke`、`renderer.wgpu_surface`、`decode.wmf.audio`、`decode.wmf.video_first_frame`、`audio.wasapi` 和 `save.known_folder_rw` required smoke |
| `S6-LINUX-HOST-01` | Linux | `SPEC_READY` | 计划补 winit/wgpu、IME、gamepad、PipeWire/PulseAudio、XDG data、GStreamer/FFmpeg profile 和 windowed smoke |
| `S6-MACOS-HOST-01` | macOS | `SPEC_READY` | 计划补 AppKit/winit lifecycle、Metal/wgpu、IME/gamepad、CoreAudio、App Support、AVFoundation 和 notarization-relevant capability |

## Capability

Windows 输出 WMF/WASAPI/DPI/IME/gamepad capability，并在 smoke 中记录 hidden window、wgpu surface、WMF audio/video decode、WASAPI stream 和 Known Folder RW evidence。Linux 输出 window system、audio backend、fontconfig、GStreamer 或 FFmpeg profile；macOS 输出 AppKit bridge、AVFoundation、sandbox path 和 notarization-relevant metadata。Linux/macOS 字段目前是计划口径，不代表真实 host 已完成。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。

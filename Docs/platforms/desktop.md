# Desktop Platforms

Desktop host 覆盖 Windows、Linux、macOS。窗口和输入默认使用 winit，渲染默认 wgpu，解码优先平台 API，FFmpeg/vcpkg 作为 fallback。

对应 crate 是 `astra-platform-windows`、`astra-platform-linux` 和 `astra-platform-macos`。Migration 8 使用 `astra.platform_capability_report.v2` 与 host conformance gate；三个桌面平台当前均为 `IN_PROGRESS`。

## Responsibilities

- 创建 surface、处理 DPI、窗口、输入法、手柄、文件选择、权限和 crash bundle。
- 提供 platform decode provider、filesystem provider、secret provider 和 system integration。
- 不保存 Runtime state，不直接调用 Actor 或 StateMachine 内部结构。

## Release Gate

每个平台必须跑 package launch、headless scenario、windowed smoke、save/load/replay、audio output probe、decode fallback 和 plugin fingerprint check。

## Current Status

| Work ID | Platform | Status | Scope |
| --- | --- | --- | --- |
| `S2-WINDOWS-HOST-01` | Windows | `IN_PROGRESS` | winit real window、shared hardware wgpu present/readback、DPI/IME/input、WGI gamepad、WASAPI format query/pause/resume/abort/telemetry、WMF、FFmpeg native media session、Saved Games transaction、rfd user-authorized source 和 streaming HTTPS verified cache 已接入；剩余 cache lease/LRU、Player 接线、性能预算与正式 conformance |
| `S2-WINDOWS-WMF-01` | Windows | `DONE` | WMF `DecodeProvider` 对 CC0 public fixture 执行 `decode.wmf.audio` MP3 PCM 和 `decode.wmf.video_first_frame` MP4 BGRA 首帧；失败返回 blocking diagnostic |
| `S2-WINDOWS-MEDIA-SESSION-01` | Windows | `IN_PROGRESS` | 显式 `[wmf, ffmpeg]` fallback profile 可运行 FFmpeg timestamped stream→audio-master scheduler→WASAPI/wgpu，真实测试覆盖视觉、非静音音频 meter、pause/resume、seek、backpressure、device-loss recovery、资源释放和 profile-bound measured performance report；正式 release-reference performance pass 与 Player/package identity 尚未闭合 |
| `S2-WINDOWS-GATE-01` | Windows | `IN_PROGRESS` | Release Gate 已要求 capability v2、host conformance 和 Player automation identity continuity；正式同 run evidence 尚未通过 |
| `S6-LINUX-HOST-01` | Linux | `IN_PROGRESS` | Steam Linux Runtime 3.0 sniper x86_64 profile、Wayland/winit、Vulkan/wgpu hardware-only、ALSA/cpal、GStreamer、XDG data/portal、IME/gamepad、save/package 和 packaged Player 已接线；等待真实 host、媒体、输入与同 run evidence |
| `S6-MACOS-HOST-01` | macOS | `IN_PROGRESS` | 主线程 AppKit/winit runner、Metal/wgpu、IME/gamepad、CoreAudio、Application Support、AVFoundation、Universal 2 `.app` 和 packaged Player 已接线；两个 Apple target 已通过本机 Cargo 静态编译，等待真实 macOS evidence |

## Capability

Desktop capability v2 只在 live conformance 中把真实可用 provider 写入 `available`/`selected`。普通 `astra platform probe` 不执行硬件验收，因此不会把接口存在性当成可用证据。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。

## macOS Development Dependencies

macOS 静态目标是 macOS 13 Universal 2。`Tools/run_macos_cargo.py` 直接调用本机 Cargo，默认读取 `/usr/local/osx-ndk-x86`，也可通过 `ASTRA_OSXCROSS_ROOT` 指定完整、可重定位的 osxcross。工具链必须提供 `bin/o64-clang`、`bin/oa64-clang`、对应的 `ar` 和 `SDK/MacOSX13.3.sdk`，Rust 必须安装两个 Apple target。脚本会在执行前逐项校验，并把 build identity 绑定到独立 target root；缺项时直接停止，不使用容器或共享 target。Apple SDK 不得提交到仓库。

当前只执行两个 Apple target 的 `check` 和 `clippy`。AppKit、Metal、CoreAudio、AVFoundation、CGEvent、ScreenCaptureKit、AccessKit、codesign、notarization 和 Player 同 run evidence 必须在 macOS 13+ 真机补齐。本机 Cargo 交叉编译只计 E1。

## Linux Development Dependencies

Linux 发布目标固定为 x86_64 Steam Linux Runtime 3.0 sniper。开发机需要 Rust stable、Wayland、Vulkan loader 与硬件 ICD、ALSA、udev、GStreamer base/good/bad/ugly/libav、XDG Desktop Portal，以及可选的 Fcitx5 和 `/dev/uinput` 测试权限。Arch Linux 对应包如下：

```bash
sudo pacman -S rust wayland libxkbcommon vulkan-icd-loader alsa-lib systemd-libs \
  gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly \
  gst-libav xdg-desktop-portal xdg-desktop-portal-gtk fcitx5
```

Vulkan ICD 按 GPU 选择 `vulkan-intel`、`vulkan-radeon`、`nvidia-utils`；WSL2 使用 `vulkan-dzn`。静态开发可只执行 `cargo check`、`clippy` 和 contract test；窗口、GPU、ALSA、portal、IME、gamepad、uinput、媒体播放和 E3 evidence 必须留到原生环境完整后执行。X11、PipeWire/PulseAudio native provider、AT-SPI、Linux crash reporter、UI component 和 Steamworks 不在当前范围。

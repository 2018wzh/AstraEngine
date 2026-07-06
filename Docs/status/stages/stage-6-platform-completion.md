# Stage 6 Platform Completion Work

Stage 6 收纳 Stage 2 之外的平台完成项。Windows 和 Web 属于 Stage 2 完成边界；Linux、macOS、iOS 和 Android 保持 `SPEC_READY`，等 AstraVN Core、Editor gate 和平台发布路径稳定后再进入真实 SDK、launcher、surface、media、save 和 resume 验收。本页不把 capability crate 编译通过写成 host 完成。

## S6-LINUX-HOST-01 Linux host completion

**ID:** `S6-LINUX-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 Linux window/input/audio/save/decode probe，覆盖 winit/wgpu、IME、gamepad、PipeWire/PulseAudio、XDG data、GStreamer/FFmpeg profile 和 windowed smoke。

**Depends On:** `S2-PLATFORM-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-linux/`、`Docs/platforms/desktop.md`

**Planned Gate:** required smoke 暂定 `windowed_smoke` 和 `decode.linux_media`。进入实现时必须提供真实 Linux host evidence；缺 SDK 或缺 smoke 只能进入 blocking 或 warning report。

**Linked Test IDs:** `T-S6-LINUX-HOST-01`

## S6-MACOS-HOST-01 macOS host completion

**ID:** `S6-MACOS-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 macOS AppKit/winit lifecycle、Metal/wgpu、IME/gamepad、CoreAudio、App Support save store、AVFoundation decode 和 notarization-relevant capability。

**Depends On:** `S2-PLATFORM-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-macos/`、`Docs/platforms/desktop.md`

**Planned Gate:** required smoke 暂定 `windowed_smoke` 和 `decode.avfoundation`。release profile 必须读取真实 platform report，不接受环境变量伪造 SDK evidence。

**Linked Test IDs:** `T-S6-MACOS-HOST-01`

## S6-IOS-HOST-01 iOS host completion

**ID:** `S6-IOS-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 Swift/SwiftUI launcher、Metal surface、safe area/touch、AVAudio/AVFoundation、app container save、no-JIT Luau gate 和 foreground/background resume。

**Depends On:** `S2-PLATFORM-01`、`S3-LUAU-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-ios/`、`Docs/platforms/mobile.md`

**Planned Gate:** required smoke 暂定 `launcher_smoke` 和 `decode.avfoundation`。Luau policy 必须走 no-JIT profile，package import 和 save persistence 需要设备或模拟器 evidence。

**Linked Test IDs:** `T-S6-IOS-HOST-01`

## S6-ANDROID-HOST-01 Android host completion

**ID:** `S6-ANDROID-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 Kotlin/Java launcher、Vulkan/wgpu surface、touch/safe area、AAudio/OpenSL ES、MediaCodec、SAF/package import、activity resume 和 no-JIT Luau gate。

**Depends On:** `S2-PLATFORM-01`、`S3-LUAU-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-android/`、`Docs/platforms/mobile.md`

**Planned Gate:** required smoke 暂定 `launcher_smoke` 和 `decode.mediacodec`。实现时必须验证 package source、activity lifecycle、save store 和 audio focus。

**Linked Test IDs:** `T-S6-ANDROID-HOST-01`

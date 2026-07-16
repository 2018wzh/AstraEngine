# Stage 6 Platform Completion Work

Stage 6 收纳 Stage 2 之外的平台完成项。Linux、macOS host 与 packaged Player 已进入 `IN_PROGRESS`；iOS 和 Android 保持 `SPEC_READY`。Linux/macOS Headless 的本机 CI、runtime 与 artifact portability 在本阶段独立关闭。本页不把 capability crate 编译通过写成 host 完成，也不把 API 可用性写成可玩证据。

## S6-LINUX-HEADLESS-01 Linux Headless portability

**ID:** `S6-LINUX-HEADLESS-01`

**Status:** `IN_PROGRESS`

**Goal:** 在 Linux native 环境复核 Migration 11 Headless 的完整 host、JSONL、真实 PNG/WAV、decode、transactional save、bounded package source、artifact、review bundle、统一测试 inventory 与 zero-leak shutdown，不以只编译通过替代 runtime evidence。

**Depends On:** `S2-HEADLESS-TEST-MIGRATION-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-headless/`、`Engine/Source/Programs/astra-headless/`、`.github/workflows/ci.yml`

**Planned Gate:** 独立 Linux native CI 必须执行默认 workspace test、Headless convergence、shipping graph 和公开产品 artifact run；视频 provider 另设显式 FFmpeg job。缺系统依赖、link/runtime failure 或 artifact identity drift 都必须 blocking。

**Linked Test IDs:** `T-S6-LINUX-HEADLESS-01`

## S6-MACOS-HEADLESS-01 macOS Headless portability

**ID:** `S6-MACOS-HEADLESS-01`

**Status:** `SPEC_READY`

**Goal:** 在 macOS native 环境复核 Migration 11 Headless 的完整 host、JSONL、真实 PNG/WAV、decode、transactional save、bounded package source、artifact、review bundle、统一测试 inventory 与 zero-leak shutdown。

**Depends On:** `S2-HEADLESS-TEST-MIGRATION-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-headless/`、`Engine/Source/Programs/astra-headless/`、`.github/workflows/ci.yml`

**Planned Gate:** 独立 macOS native CI 必须执行默认 workspace test、Headless convergence、shipping graph 和公开产品 artifact run；不得用 Windows report、cross compile 或静态 schema 代替 macOS runtime evidence。

**Linked Test IDs:** `T-S6-MACOS-HEADLESS-01`

## S6-LINUX-HOST-01 Linux host completion

**ID:** `S6-LINUX-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 完成 Steam Linux Runtime 3.0 sniper x86_64 的 Wayland window/input、Vulkan hardware wgpu、ALSA、XDG data/portal、GStreamer decode、IME/gamepad、save/package 和 windowed smoke。

**Depends On:** `S2-PLATFORM-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-linux/`、`Docs/platforms/desktop.md`

**Current Evidence:** `astra-platform-linux` 已接入真实 factory、typed resource lifecycle、Wayland event loop、Vulkan-only wgpu、ALSA/cpal、GStreamer、XDG save/portal、package cache、gamepad 和 uinput test driver，并通过静态编译与 contract test。当前按开发约束不执行真实 host。

**Planned Gate:** required checks 以 `required_conformance_checks(PlatformId::Linux)` 为准。必须补真实 Vulkan present/readback、ALSA meter、GStreamer fixtures、portal、IME/gamepad/uinput consumption、save reopen、package hash/range 和 zero-leak evidence；缺项保持 blocking。

**Linked Test IDs:** `T-S6-LINUX-HOST-01`

## S6-LINUX-PLAYER-AUTOMATION-01 Linux player automation

**ID:** `S6-LINUX-PLAYER-AUTOMATION-01`

**Status:** `IN_PROGRESS`

**Goal:** 补 Linux player live input automation，覆盖 Wayland focus、uinput mouse/keyboard/IME/gamepad、winit event receipt、Vulkan frame region hash、ALSA meter 和 route/system UI evidence。

**Depends On:** `S6-LINUX-HOST-01`、`S3-PLAYER-AUTOMATION-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-linux/`、`Engine/Source/Programs/astra-player/` planned target、`Docs/platforms/desktop.md`

**Current Evidence:** packaged Player 已读取 Linux profile 并复用 Windows 的 RuntimeWorld/StateMachine、package、save/load、presentation 与 audio 主循环；CLI 可生成含 `astra-player`、config、package 和 manifest 的 Linux depot tree；automation validator 已接受并校验 uinput transcript。真实输入与 same-run report 尚未执行。

**Planned Gate:** `player.full_playable.linux` 必须读取 Linux host report 和 live input transcript；缺 focus、native input receipt、frame region change、audio meter 或 route evidence 时 blocking。

**Linked Test IDs:** `T-S6-LINUX-PLAYER-AUTOMATION-01`

## S6-MACOS-HOST-01 macOS host completion

**ID:** `S6-MACOS-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 macOS AppKit/winit lifecycle、Metal/wgpu、IME/gamepad、CoreAudio、App Support save store、AVFoundation decode 和 notarization-relevant capability。

**Depends On:** `S2-PLATFORM-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-macos/`、`Docs/platforms/desktop.md`

**Current Evidence:** `astra-platform-macos` 已有显式主线程 runner/factory、typed resource lifecycle、Metal-only wgpu、CoreAudio/cpal、AVFoundation CPU decode、Application Support save、package cache 和 macOS packaged Player。CLI 可生成 macOS 13 Universal 2 `.app`，并把 codesign/notarization 留作外部 blocking check。使用 macOS SDK 13.3 的本机 Cargo x86_64/aarch64 静态 check 已通过；尚未取得真机 evidence。

**Planned Gate:** required checks 以 `required_conformance_checks(PlatformId::Macos)` 为准。必须补 AppKit lifecycle、Metal present/readback、CGEvent/IME/gamepad、ScreenCaptureKit、AccessKit、CoreAudio meter、AVFoundation fixtures、save reopen、package、zero-leak、签名与公证 evidence。

**Planned Gate:** required smoke 暂定 `windowed_smoke` 和 `decode.avfoundation`。release profile 必须读取真实 platform report，不接受环境变量伪造 SDK evidence。

**Linked Test IDs:** `T-S6-MACOS-HOST-01`

## S6-MACOS-PLAYER-AUTOMATION-01 macOS player automation

**ID:** `S6-MACOS-PLAYER-AUTOMATION-01`

**Status:** `SPEC_READY`

**Goal:** 补 macOS player live input automation，覆盖 AppKit/winit window focus、native mouse/keyboard/IME/gamepad 输入、event-loop receipt、Metal/wgpu frame region hash、CoreAudio meter 和 route/system UI evidence。

**Depends On:** `S6-MACOS-HOST-01`、`S3-PLAYER-AUTOMATION-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-macos/`、`Engine/Source/Programs/astra-player/` planned target、`Docs/platforms/desktop.md`

**Current Evidence:** packaged Player 已接入 macOS profile、主线程 host runner、package、Runtime/provider、presentation、audio 和 save 主循环。`PlayerPlatform::Macos` 已锁定 CGEvent input source 与 `player_host.trace` consumption；当前没有真机自动化 report。

**Planned Gate:** `player.full_playable.macos` 必须读取 macOS host report 和 live input transcript；缺 native input、frame region change、CoreAudio meter、App Support save evidence 或 route evidence 时 blocking。

**Linked Test IDs:** `T-S6-MACOS-PLAYER-AUTOMATION-01`

## S6-IOS-HOST-01 iOS host completion

**ID:** `S6-IOS-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 Swift/SwiftUI launcher、Metal surface、safe area/touch、AVAudio/AVFoundation、app container save、no-JIT Luau gate 和 foreground/background resume。

**Depends On:** `S2-PLATFORM-01`、`S3-LUAU-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-ios/`、`Docs/platforms/mobile.md`

**Planned Gate:** required smoke 暂定 `launcher_smoke` 和 `decode.avfoundation`。Luau policy 必须走 no-JIT profile，package import 和 save persistence 需要设备或模拟器 evidence。

**Linked Test IDs:** `T-S6-IOS-HOST-01`

## S6-IOS-PLAYER-AUTOMATION-01 iOS player automation

**ID:** `S6-IOS-PLAYER-AUTOMATION-01`

**Status:** `SPEC_READY`

**Goal:** 补 iOS player live input automation，覆盖设备或模拟器 touch/keyboard 输入、safe area、foreground/background resume、Metal frame region hash、AVAudio meter、package source 和 route/system UI evidence。

**Depends On:** `S6-IOS-HOST-01`、`S3-PLAYER-AUTOMATION-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-ios/`、`Engine/Source/Programs/astra-player/` planned target、`Docs/platforms/mobile.md`

**Planned Gate:** `player.full_playable.ios` 必须使用设备或模拟器证据；缺 touch transcript、safe area evidence、frame region change、AVAudio meter、resume 或 route evidence 时 blocking。

**Linked Test IDs:** `T-S6-IOS-PLAYER-AUTOMATION-01`

## S6-ANDROID-HOST-01 Android host completion

**ID:** `S6-ANDROID-HOST-01`

**Status:** `SPEC_READY`

**Goal:** 补 Kotlin/Java launcher、Vulkan/wgpu surface、touch/safe area、AAudio/OpenSL ES、MediaCodec、SAF/package import、activity resume 和 no-JIT Luau gate。

**Depends On:** `S2-PLATFORM-01`、`S3-LUAU-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-android/`、`Docs/platforms/mobile.md`

**Planned Gate:** required smoke 暂定 `launcher_smoke` 和 `decode.mediacodec`。实现时必须验证 package source、activity lifecycle、save store 和 audio focus。

**Linked Test IDs:** `T-S6-ANDROID-HOST-01`

## S6-ANDROID-PLAYER-AUTOMATION-01 Android player automation

**ID:** `S6-ANDROID-PLAYER-AUTOMATION-01`

**Status:** `SPEC_READY`

**Goal:** 补 Android player live input automation，覆盖设备或 emulator touch/keyboard 输入、activity lifecycle、Vulkan/wgpu frame region hash、audio focus/meter、SAF/package source 和 route/system UI evidence。

**Depends On:** `S6-ANDROID-HOST-01`、`S3-PLAYER-AUTOMATION-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-android/`、`Engine/Source/Programs/astra-player/` planned target、`Docs/platforms/mobile.md`

**Planned Gate:** `player.full_playable.android` 必须使用设备或 emulator 证据；缺 touch transcript、activity resume、frame region change、audio focus/meter、package source 或 route evidence 时 blocking。

**Linked Test IDs:** `T-S6-ANDROID-PLAYER-AUTOMATION-01`

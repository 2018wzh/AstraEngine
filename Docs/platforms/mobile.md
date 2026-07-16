# Mobile Platforms

iOS 和 Android 使用独立平台模块。移动壳负责原生生命周期、surface、触控、safe area、文件导入、权限、后台音频策略和平台媒体解码。

对应 crate 是 `astra-platform-ios` 和 `astra-platform-android`。没有 SDK 时只输出缺失 SDK 的 capability report，不把移动平台标为完成。

## Current Status

| Work ID | Platform | Status | Scope |
| --- | --- | --- | --- |
| `S6-IOS-HOST-01` | iOS | `SPEC_READY` | 计划补 Swift/SwiftUI launcher、Metal surface、safe area/touch、AVAudio/AVFoundation、app container save、no-JIT Luau gate 和 foreground/background resume |
| `S6-ANDROID-HOST-01` | Android | `IN_PROGRESS` | GameActivity/Gradle、固定 toolchain、真实 Runtime/provider Player、Vulkan surface、AAudio、MediaCodec、save、bundled/SAF/HTTPS package、TalkBack tree/action 和原生输入后端已接通；API 28/36 emulator、arm64 真机与正式 E3 尚未闭合 |

## iOS

- Swift/SwiftUI launcher + Rust staticlib。
- AVFoundation decode provider 优先。
- iOS 禁止 JIT，因此 Luau 以解释执行进入 AstraVN policy；legacy EMU bridge 也不能依赖 JIT。

## Android

- Kotlin `GameActivity` 薄壳 + Rust `cdylib`；gameplay authority 只在 Rust Runtime/provider session。
- release profile 固定 `wgpu_vulkan`、`mediacodec`、`android_app_storage`，音频优先 `oboe_aaudio`，`oboe_opensl_es` 只能由 compatibility profile 明确允许并报告实际 backend。
- bundled `.astrapkg` 保持 uncompressed 并在进入 verified cache 前校验 hash；Storage Access Framework 只用 `ACTION_OPEN_DOCUMENT`、持久读权限和内容流复制。
- Activity 主线程持有 winit event loop 和 Vulkan surface。Rust Player 在线程启动后打开真实 Runtime/provider session，产品呈现只走 `PresentScene`；surface loss、暂停/恢复、旋转、capture、typed handle 和 shutdown leak check 都由 Android host 管理。
- Oboe stream 必须证明实际使用 AAudio。音频 callback 只消费有界队列并更新 meter/underflow；audio focus、duck、暂停和设备断开通过 typed event 回到 Player。
- 音频和视频使用 API 28 asynchronous MediaCodec callback。输入经 app-private scratch file 和 `AMediaExtractor` 读取，输出队列有上限；视频通过 `AImageReader` 取得完整 YUV frame、PTS/EOS，再编码为共享 `DecodedVideoStream`，不会把 first frame 当作完成。
- TalkBack 使用 Android AccessKit adapter 映射 `SceneFrame.semantics`，action 通过有界队列回送 `AccessibilityAction`。GameActivity 只转发 insets、audio focus、SAF 和 gamepad DTO，不持有 gameplay state。
- 固定 minSdk 28、compileSdk/targetSdk 36、Build Tools 36.0.0、NDK 30.0.15729638、AGP 9.3.0、Gradle 9.5.0、JDK 17。bundle identity 还要绑定实际 JDK 版本和 JDK、Build Tools、NDK Clang、Gradle wrapper 的 hash。shipping ABI 仅 `arm64-v8a`，`x86_64` 只用于 emulator。

## Testing

移动 release gate 至少覆盖启动、旋转/resize、触控、音频焦点、save/load、package import 和 foreground/background resume。

Android 的实现已进入设备验收阶段，但仍不能标为 `DONE`。目前的 cross-build、Gradle 和静态检查只算实现与构建证据。只有 API 28/36 emulator 与至少一台 arm64 Vulkan 真机在同一 build/package/profile/session/input identity 下完成 Vulkan、MediaCodec、AAudio、SAF、save/recreate、TalkBack、Player route 和人工视觉/音频 review，才达到 E3。

## Capability

iOS 和 Android 都必须报告 safe area、touch profile、audio focus/background policy、package import source、save persistence、platform decode、network permission 和 Luau no-JIT mode。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。

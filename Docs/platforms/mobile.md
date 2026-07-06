# Mobile Platforms

iOS 和 Android 使用独立平台模块。移动壳负责原生生命周期、surface、触控、safe area、文件导入、权限、后台音频策略和平台媒体解码。

对应 crate 是 `astra-platform-ios` 和 `astra-platform-android`。没有 SDK 时只输出缺失 SDK 的 capability report，不把移动平台标为完成。

## iOS

- Swift/SwiftUI launcher + Rust staticlib。
- AVFoundation decode provider 优先。
- iOS 禁止 JIT，因此 Luau 以解释执行进入 AstraVN policy；legacy EMU bridge 也不能依赖 JIT。

## Android

- Kotlin/Java launcher + Rust cdylib。
- MediaCodec decode provider 优先。
- Storage Access Framework 只提供用户授权目录。

## Testing

移动 release gate 至少覆盖启动、旋转/resize、触控、音频焦点、save/load、package import 和 foreground/background resume。

## Capability

iOS 和 Android 都必须报告 safe area、touch profile、audio focus/background policy、package import source、save persistence、platform decode、network permission 和 Luau no-JIT mode。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。

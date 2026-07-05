# Mobile Platforms

iOS 和 Android 使用独立平台模块。移动壳负责原生生命周期、surface、触控、safe area、文件导入、权限、后台音频策略和平台媒体解码。

## iOS

- Swift/SwiftUI launcher + Rust staticlib。
- AVFoundation decode provider 优先。
- iOS 禁止 JIT，因此 LuaJIT 不作为默认方案；Lua 5.4 解释执行。

## Android

- Kotlin/Java launcher + Rust cdylib。
- MediaCodec decode provider 优先。
- Storage Access Framework 只提供用户授权目录。

## Testing

移动 release gate 至少覆盖启动、旋转/resize、触控、音频焦点、save/load、package import 和 foreground/background resume。

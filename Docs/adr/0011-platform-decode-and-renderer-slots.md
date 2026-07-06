# ADR 0011: 平台解码优先，Renderer2D provider 可替换

## Context

视频、音频和图片解码在不同平台有不同硬件路径。渲染后端也受 Web、移动和实验平台限制。

## Decision

DecodeProvider 优先使用平台原生能力，桌面用 FFmpeg/vcpkg 作为 fallback。Renderer2D 通过 provider slot 选择，wgpu 为默认实现。

## Consequences

平台模块可以充分使用硬件加速。公共 API 只传递 `MediaSurfaceToken` 或 CPU buffer，不暴露 native handle。

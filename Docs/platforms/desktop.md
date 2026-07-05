# Desktop Platforms

Desktop host 覆盖 Windows、Linux、macOS。窗口和输入默认使用 winit，渲染默认 wgpu，解码优先平台 API，FFmpeg/vcpkg 作为 fallback。

## Responsibilities

- 创建 surface、处理 DPI、窗口、输入法、手柄、文件选择、权限和 crash bundle。
- 提供 platform decode provider、filesystem provider、secret provider 和 system integration。
- 不保存 Runtime state，不直接调用 Actor 或 StateMachine 内部结构。

## Release Gate

每个平台必须跑 package launch、headless scenario、windowed smoke、save/load/replay、audio output probe、decode fallback 和 plugin fingerprint check。

# Architecture Decision Records

ADR 记录已经锁定的产品级决策。被替换的旧决策必须在新 ADR 中说明取代关系。

| ADR | 决策 |
| --- | --- |
| [0001](0001-modular-2d-engine-baseline.md) | 模块化 2D/VN-first 引擎基线 |
| [0002](0002-editor-ui-qt.md) | Editor 使用 Qt/QML + Rust core |
| [0003](0003-renderer2d-first-stage-backend.md) | Renderer2D 后端可替换，wgpu 为默认 provider |
| [0004](0004-actor-component-statemachine-runtime.md) | Actor/Component + StateMachine 是 Runtime 权威模型 |
| [0005](0005-mcp-agent-capability-protocol.md) | MCP 是受权限和审计约束的能力协议 |
| [0006](0006-text-first-source-data.md) | Text-first source，binary runtime data |
| [0008](0008-rust-plugin-abi.md) | 插件采用 Rust-facing `abi_stable` 风格 ABI |
| [0009](0009-astraemu-out-of-process-core.md) | Superseded: AstraEMU compat core 独立进程化 |
| [0010](0010-state-machine-action-provider.md) | StateMachine action provider 采用 host context 与 FFI effect list |
| [0011](0011-platform-decode-and-renderer-slots.md) | 平台解码优先，Renderer2D provider 可替换 |
| [0012](0012-astraemu-engine-native-family-plugin.md) | AstraEMU 使用 engine-native family plugin + LegacyRuntimeProvider facade |
| [0013](0013-astravn-script-frontend-standardization.md) | AstraVN script v1 主线标准化编译器前端，不引入 Cranelift/native codegen |
| [0014](0014-stable-rust-toolchain-policy.md) | 使用 stable toolchain、lockfile 和构建身份代替未经验证的固定 MSRV |
| [0015](0015-ui-backend-provider-split.md) | AstraVN 使用 Yakui，AstraEMU 使用 Slint，Editor 保持 Qt/QML |
| [0016](0016-astravn-script-declared-ui.md) | `.astra` 声明 backend-neutral UI，Rust ViewModel 与 Luau Controller 保持权威分层 |
| [0017](0017-ui-component-plugin-boundary.md) | 独立 UI component ABI、typed slot、签名与资源上限 |

# AstraEngine 产品愿景

AstraEngine 是 Rust + WGPU-first 的 2D/VN-first 高性能游戏引擎。它的核心不是“复制 UE”，而是在视觉小说、互动叙事和演出强化这个范围内，达到 UE 去掉 3D 大型玩法后的工程成熟度：可独立发布、可调试、可回放、可扩展、可审计。

## 用户

| 用户 | 需要完成的事 | 产品承诺 |
| --- | --- | --- |
| 创作者 | 从模板创建 VN 项目，导入素材，编写 `.astra`，编辑 Graph/Timeline，PIE 调试，打包发布 | 不理解底层 runtime 也能完成完整项目 |
| 开发者 | 扩展 renderer、audio、text layout、script runtime、presentation library、asset importer、editor panel、AI/MCP tool | 通过稳定插件 ABI 和 schema 接入，不改 EngineCore |
| 平台维护者 | 让同一 packaged runtime 在桌面、移动、Web 和实验平台运行 | 平台模块只适配 surface、输入、权限、生命周期和平台解码 |
| 旧 VN 研究者 | 在合法本地数据上实现兼容、现代化、翻译和补丁流程 | AstraEMU 作为独立套件复用引擎能力，不污染 NativeVN |

## 硬目标

- EngineCore 使用 Actor/Component + StateMachine 驱动 deterministic runtime。
- Runtime 可脱离 Editor 完成 launch、tick、save、load、replay、diagnostics、profiling 和 release validation。
- AstraVN 使用 `.astra` 作为 canonical story source，Luau policy 用于扩展和受控演出策略。
- Editor 使用 Qt/QML + Rust core，覆盖完整 creator workflow。
- 插件采用 Rust-facing `abi_stable` 风格 ABI，支持加载/卸载和 provider selection，不支持热重载。
- 平台硬目标是 Windows、Linux、macOS、iOS、Android、Web；旧主机/掌机是实验模块。
- Runtime AI 可以发布，但所有 committed AI output 必须进入 save/replay，不允许回放时重新请求 provider。
- AstraEMU 使用独立进程 core，v1 可用 family 是 Artemis；KrKr、BGI、SoftPAL、FVP、Siglus 输出 alpha probe report 后逐步实现。

## 非目标

- 不追求复杂 3D、FPS、高实时网络竞技、大型开放世界 streaming 或 UE full object model。
- 不把 Editor、AI provider、MCP server、Luau runtime、legacy VM、平台图形句柄放进 Core 依赖。
- 不把旧 VN 导入为 Astra canonical source。
- 不在文档或工具中提供绕过 DRM、商业保护或访问控制的方案。

## v1 Definition

v1 不是单个 crate 可编译，而是全系列可发布闭环：EngineCore native smoke、AstraVN commercial baseline、AstraEditor creator workflow、Windows/Linux/macOS/iOS/Android/Web profile gate、AI/MCP audit 和 Artemis full-flow report 都通过 Release Gate。

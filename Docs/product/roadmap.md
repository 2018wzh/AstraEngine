# 路线图

路线图按 Stage Gate 管理。每个 Stage 都有独立工作清单、测试矩阵映射和退出标准；产品页只保存阶段目标，不记录当前实现状态。

## Stage Gates

| Stage | 目标 | 退出标准 | Work |
| --- | --- | --- | --- |
| Stage 1 EngineCore | RuntimeWorld、Actor/Component、StateMachine、EventBus、Scheduler、AwaitToken、Save/Replay、PropertySystem、Plugin ABI、Target manifest 和 headless scenario runner。 | Native smoke 可 headless run、save/load/replay，Runtime determinism、Target validation 和插件 fingerprint/load-unload gate 通过；package build 留 Stage 2。 | [stage-1-enginecore](../status/stages/stage-1-enginecore.md) |
| Stage 2 Media + Package | Import/Cook、binary package、Asset VFS mount family、Renderer2D slot、TextLayout、AudioGraph、FilterGraph、DecodeProvider、Windows/Web Platform capability、strict scenario runner 和 release report。 | Package integrity、VFS package/local authorized/legacy pack/overlay mount gate、provider eligibility、headless capture、decode fallback、flat StateMachine、Await/Fence、target manifest、Windows/Web platform report 和 release report schema 通过；desktop-release/web-release 缺 platform report 时阻断。 | [stage-2-media-package](../status/stages/stage-2-media-package.md) |
| Stage 3 AstraVN | `.astra` 编译、AstraVN module layout、多功能 crate 拆分、facade-only `astra-vn` Rust dylib、`NativeVnRuntimeProvider`、Luau policy、Graph/Timeline 同源、NativeVN Game target、商业 VN 系统 UI、标准命令库、演出模型、完整 playthrough scenario 和 Windows/Web live player automation。 | AstraVN 迁到 `Engine/Source/Modules/AstraVN/`；`astra-vn` 只产出 `rlib`/Rust ABI `dylib` 和 re-export；`.astra` sample 完成 dialogue、choice、backlog、auto、skip、save/load、config、video、system UI、advanced presentation opt-in、Game target、runtime provider binding 和 replay hash gate；Windows/Web `player.full_playable` 必须由平台原生输入、input transcript、视觉变化、音频 meter 和 host evidence 证明。 | [stage-3-astra-vn](../status/stages/stage-3-astra-vn.md) |
| Stage 4 Editor + AI/MCP | Qt/QML editor、Editor target、PIE、Inspector、Debugger、Package panel、Plugin Manager、Runtime Director、AI provider profile、Asset VFS-backed ONNX ModelBundle、Editor Copilot、Content Generation、Context Pack、runtime memory 和 audit。 | Project Wizard 到 Package/Release Gate 闭环可用，Editor target 隔离、插件启用诊断、Trusted session、Review Queue、provider profile、Asset VFS-backed ONNX ModelBundle、memory policy、context permission、provider-free replay 和 audit gate 通过。 | [stage-4-editor-ai-mcp](../status/stages/stage-4-editor-ai-mcp.md) |
| Stage 5 AstraEMU | Program target、Manager + `AstraEmuRuntimeProvider` + RuntimeWorld、`LegacyRuntimeProvider` facade、EmulatorCore 状态机映射、legacy pack VFS、auto probe、Trusted Luau、文本翻译、FilterGraph preset、Artemis 通用 family plugin、其他 family alpha profile。 | Manager 以 Program target 启动 gameplay runtime session，Artemis full-flow gate、provider session snapshot/replay、VM scheduler/context trace、legacy pack VFS report、auto probe report、trusted script isolation、text redaction 和 filter preset gate 通过。 | [stage-5-astra-emu](../status/stages/stage-5-astra-emu.md) |
| Stage 6 Platform Completion | Linux、macOS、iOS 和 Android host completion，覆盖真实 SDK、launcher/window、surface、platform decode、audio、save store、package source、resume、平台输入自动化和 release evidence。 | 四个平台分别提供真实 host smoke、player input transcript、frame region、audio meter、route evidence 和 release profile report；缺 SDK、缺 required smoke、缺 package/source evidence 或缺平台输入自动化不能写成 release pass。 | [stage-6-platform-completion](../status/stages/stage-6-platform-completion.md) |

## 测试矩阵

所有 Stage work 的测试项目统一维护在 [stage-test-matrix](../status/stages/stage-test-matrix.md)。新增或调整 Stage work 时，必须同步更新测试矩阵和最近的状态索引。

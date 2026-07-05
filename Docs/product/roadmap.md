# 路线图

路线图按 Stage Gate 管理。每个 Stage 都有独立工作清单、测试矩阵映射和退出标准；产品页只保存阶段目标，不记录当前实现状态。

## Stage Gates

| Stage | 目标 | 退出标准 | Work |
| --- | --- | --- | --- |
| Stage 1 EngineCore | RuntimeWorld、Actor/Component、StateMachine、EventBus、Scheduler、AwaitToken、Save/Replay、PropertySystem、Plugin ABI 和 headless scenario runner。 | Native sample 可 package、headless run、save/load/replay，Runtime determinism 和插件 fingerprint gate 通过。 | [stage-1-enginecore](../status/stages/stage-1-enginecore.md) |
| Stage 2 Media + Package | Import/Cook、binary package、Renderer2D slot、TextLayout、AudioGraph、FilterGraph、DecodeProvider 和 release report。 | Package integrity、provider eligibility、headless capture、decode fallback 和 release report schema 通过。 | [stage-2-media-package](../status/stages/stage-2-media-package.md) |
| Stage 3 AstraVN | `.astra` 编译、Luau policy、Graph/Timeline 同源、商业 VN 系统 UI 和完整 playthrough scenario。 | `.astra` sample 完成 dialogue、choice、backlog、auto、skip、save/load、config、video 和 replay hash gate。 | [stage-3-astra-vn](../status/stages/stage-3-astra-vn.md) |
| Stage 4 Editor + AI/MCP | Qt/QML editor、PIE、Inspector、Debugger、Package panel、Runtime AI、Editor Copilot、Content Generation 和 audit。 | Project Wizard 到 Package/Release Gate 闭环可用，Trusted session、Review Queue、provider-free replay 和 audit gate 通过。 | [stage-4-editor-ai-mcp](../status/stages/stage-4-editor-ai-mcp.md) |
| Stage 5 AstraEMU | Manager/core IPC、family API、Artemis 通用 compat core、其他 family alpha scaffold。 | Manager/core IPC、family API、Artemis full-flow gate 和其他 family probe report 通过。 | [stage-5-astra-emu](../status/stages/stage-5-astra-emu.md) |

## 测试矩阵

所有 Stage work 的测试项目统一维护在 [stage-test-matrix](../status/stages/stage-test-matrix.md)。新增或调整 Stage work 时，必须同步更新测试矩阵和最近的状态索引。

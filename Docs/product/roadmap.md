# 路线图

路线图按 Stage Gate 管理。每个 Stage 都有独立工作清单、测试矩阵映射和退出标准；产品页只保存阶段目标，不记录当前实现状态。

## Stage Gates

| Stage | 目标 | 退出标准 | Work |
| --- | --- | --- | --- |
| Stage 1 EngineCore | RuntimeWorld、Actor/Component、StateMachine、EventBus、Scheduler、AwaitToken、Save/Replay、PropertySystem、Plugin ABI、Target manifest 和 headless scenario runner。 | Native smoke 可 headless run、save/load/replay，Runtime determinism、Target validation 和插件 fingerprint/load-unload gate 通过；package build 留 Stage 2。 | [stage-1-enginecore](../status/stages/stage-1-enginecore.md) |
| Stage 2 Media + Package | Import/Cook、binary package、Renderer2D slot、TextLayout、AudioGraph、FilterGraph、DecodeProvider、Platform capability 和 release report。 | Package integrity、provider eligibility、headless capture、decode fallback、target manifest、platform capability report 和 release report schema 通过。 | [stage-2-media-package](../status/stages/stage-2-media-package.md) |
| Stage 3 AstraVN | `.astra` 编译、Luau policy、Graph/Timeline 同源、NativeVN Game target、商业 VN 系统 UI、标准命令库、演出模型和完整 playthrough scenario。 | `.astra` sample 完成 dialogue、choice、backlog、auto、skip、save/load、config、video、system UI、advanced presentation opt-in、Game target 和 replay hash gate。 | [stage-3-astra-vn](../status/stages/stage-3-astra-vn.md) |
| Stage 4 Editor + AI/MCP | Qt/QML editor、Editor target、PIE、Inspector、Debugger、Package panel、Plugin Manager、Runtime Director、AI provider profile、Editor Copilot、Content Generation、Context Pack、runtime memory 和 audit。 | Project Wizard 到 Package/Release Gate 闭环可用，Editor target 隔离、插件启用诊断、Trusted session、Review Queue、provider profile、memory policy、context permission、provider-free replay 和 audit gate 通过。 | [stage-4-editor-ai-mcp](../status/stages/stage-4-editor-ai-mcp.md) |
| Stage 5 AstraEMU | Program target、Manager + RuntimeWorld、`LegacyRuntimeProvider` facade、auto probe、Trusted Luau、文本翻译、FilterGraph preset、Artemis 通用 family plugin、其他 family alpha scaffold。 | Manager 以 Program target 驱动 RuntimeWorld，Artemis full-flow gate、provider session snapshot/replay、auto probe report、trusted script isolation、text redaction 和 filter preset gate 通过。 | [stage-5-astra-emu](../status/stages/stage-5-astra-emu.md) |

## 测试矩阵

所有 Stage work 的测试项目统一维护在 [stage-test-matrix](../status/stages/stage-test-matrix.md)。新增或调整 Stage work 时，必须同步更新测试矩阵和最近的状态索引。

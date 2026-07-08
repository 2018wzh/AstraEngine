# Stage Work

本目录把路线图拆成可执行工作项。Stage 1-2 已实现基础闭环；Stage 3-6 记录后续目标、步骤和验收证据。当前代码完成度和下一步顺序见 [implementation-plan](../implementation-plan.md)。

| 文档 | 内容 |
| --- | --- |
| [stage-1-enginecore.md](stage-1-enginecore.md) | EngineCore、Runtime、Save/Replay、Plugin ABI、Target manifest 和 headless test，已实现 |
| [stage-2-media-package.md](stage-2-media-package.md) | Asset/Cook/Package、Media provider、Platform capability 和 release report |
| [stage-3-astra-vn.md](stage-3-astra-vn.md) | `astra-vn` Rust dylib facade、`.astra`、AstraVN Core、Game target、Luau policy、standard commands、system UI、full playthrough 和 Windows/Web live player automation |
| [stage-4-editor-ai-mcp.md](stage-4-editor-ai-mcp.md) | Editor target、Editor workflow、PIE、Release Gate UI、Runtime AI 和 MCP |
| [stage-5-astra-emu.md](stage-5-astra-emu.md) | Program target、AstraEMU Manager、LegacyRuntimeProvider facade、auto probe、Trusted Luau、文本翻译和 FilterGraph preset |
| [stage-6-platform-completion.md](stage-6-platform-completion.md) | Linux、macOS、iOS 和 Android host completion 与 player input automation |
| [stage-test-matrix.md](stage-test-matrix.md) | Stage 1-6 工作项对应的测试项目 |

## Work Item 格式

每个工作项使用同一组字段：

- `ID`：`S<stage>-<area>-<number>`，例如 `S1-RUNTIME-01`。
- `Goal`：交付结果，不能写成泛泛方向。
- `Depends On`：前置工作项或 contract。
- `Target Paths`：已实现工作写实际路径；未实现工作写目标路径并保留 planned target 标记。
- `Steps`：执行级步骤，只写必要动作，不塞未来代码全文。
- `Done Evidence`：可以提交给 review 或 release gate 的证据。
- `Linked Test IDs`：必须能在 [stage-test-matrix.md](stage-test-matrix.md) 找到。
- `Status`：统一维护在 [implementation-plan.md](../implementation-plan.md)。实现完成后，先跑对应测试，再把工作项从 `NOT_STARTED` 或 `IN_PROGRESS` 改成 `DONE`。

## 维护规则

- 设计目标留在 `Docs/product`、`Docs/modules` 和 `Docs/contracts`，当前状态和缺口留在 `Docs/status`。
- 新增工作项时同步更新测试矩阵；删除工作项时移除矩阵里的引用。
- 完成工作项时同步更新 [implementation-plan.md](../implementation-plan.md)、coverage matrix 和该 Stage 的 evidence。
- 每个 Stage 的退出标准至少对应一个 release gate check。
- 文档改动后运行 `python Tools/check_docs.py`。

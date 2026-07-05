# Stage Work

本目录把路线图拆成可执行工作项。这里记录 planned target、步骤和验收证据，不表示仓库已经实现对应 crate、scenario 或工具。

| 文档 | 内容 |
| --- | --- |
| [stage-1-enginecore.md](stage-1-enginecore.md) | EngineCore、Runtime、Save/Replay、Plugin ABI 和 headless test |
| [stage-2-media-package.md](stage-2-media-package.md) | Asset/Cook/Package、Media provider 和 release report |
| [stage-3-astra-vn.md](stage-3-astra-vn.md) | `.astra`、AstraVN Core、Luau policy 和 full playthrough |
| [stage-4-editor-ai-mcp.md](stage-4-editor-ai-mcp.md) | Editor workflow、PIE、Release Gate UI、Runtime AI 和 MCP |
| [stage-5-astra-emu.md](stage-5-astra-emu.md) | AstraEMU Manager、family core 和 local case report |
| [stage-test-matrix.md](stage-test-matrix.md) | Stage 1-5 工作项对应的测试项目 |

## Work Item 格式

每个工作项使用同一组字段：

- `ID`：`S<stage>-<area>-<number>`，例如 `S1-RUNTIME-01`。
- `Goal`：交付结果，不能写成泛泛方向。
- `Depends On`：前置工作项或 contract。
- `Target Paths`：计划创建或修改的路径；尚未存在的路径写成 planned target。
- `Steps`：执行级步骤，只写必要动作，不塞未来代码全文。
- `Done Evidence`：可以提交给 review 或 release gate 的证据。
- `Linked Test IDs`：必须能在 [stage-test-matrix.md](stage-test-matrix.md) 找到。

## 维护规则

- 设计目标留在 `Docs/product`、`Docs/modules` 和 `Docs/contracts`，当前状态和缺口留在 `Docs/status`。
- 新增工作项时同步更新测试矩阵；删除工作项时移除矩阵里的引用。
- 每个 Stage 的退出标准至少对应一个 release gate check。
- 文档改动后运行 `python Tools\check_docs.py`。

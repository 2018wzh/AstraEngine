# 迁移资料

当前目标、所有权与验收只由[总体架构](../product/architecture.md)、[重构契约](../contracts/rebuild.md)和[实施状态](../status/implementation-plan.md)维护。本目录不是额外的阶段执行顺序或完成度矩阵。

已被实际切换取代的 GameRuntimeProvider、Qt Editor、延期 RPG/AI 及旧 crate 搬迁方案从活动文档删除，历史通过 Git 查询。仍保留的技术资料只解释其对应边界；接口、命令和状态与现行契约冲突时，不能按旧文档恢复已删除路径或兼容层。

## 实施入口

| 范围 | 入口 |
| --- | --- |
| workspace 与依赖 | [Workspace 组织](../implementation/workspace-blueprint.md) |
| 公共运行层与可靠存档 | [Runtime](../contracts/runtime.md)、[任务作用域](../contracts/task-scope.md) |
| NativeVN typed 会话 | [AstraVN](../modules/astra-vn.md) |
| 文本、Graph/Timeline、ACP/MCP 与预览 | [Editor 手册](../../Editor/README.md) |
| 独立 EMU Host | [Family 契约](../contracts/astraemu-ipc.md)、[客户端手册](../manual/astraemu-client.md) |
| 日常定向检查与阶段全量验证 | [开发手册](../manual/development.md) |

迁移共用契约必须同步真实调用方；只有运行过的行为才能写为通过。缺少平台环境、固定核心对象或商业源时，保留明确缺口，不创建额外治理页反复登记相同状态。

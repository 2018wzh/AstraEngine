# AstraEMU BGI 实现文档

本文档组只覆盖 AstraEMU 的 BGI/Ethornell family。目标是给 compat core 实现者提供可落地的 archive、script、VM、presentation 和 media 规则，不声明 AstraEngine 当前已经实现这些能力。

BGI core 必须仍然是 AstraEMU 的独立进程 family core：旧引擎状态机在 core 内保持权威状态，Manager 只通过本地 RPC、shared memory、trace、media block、`TextCaptureEvent` 和 snapshot 观察它。BGI 规则不能反向污染 EngineCore 的 Actor/Component 边界。

## 文档索引

| 文件 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 本轮采用的源码、工具和游戏样本清单。 |
| [archive-format.md](archive-format.md) | `PackFile`、`BURIKO ARC20`、`DSC FORMAT 1.00` 和资源探测规则。 |
| [script-format.md](script-format.md) | BGI script payload 分类、检测顺序和编码边界。 |
| [script-bcs.md](script-bcs.md) | `BurikoCompiledScriptVer1.00` header、command stream 和 source map。 |
| [script-bp.md](script-bp.md) | `._bp` system program header、bytecode 和 string pool。 |
| [vm-dispatch.md](vm-dispatch.md) | BGI VM dispatch group、参数栈和已知 host call。 |
| [script-execution.md](script-execution.md) | script load、VM tick、等待边界、save/replay 状态。 |
| [presentation-and-media.md](presentation-and-media.md) | `CompressedBG___`、raw image、audio box、movie 和 layer 规则。 |
| [runtime-core-design.md](runtime-core-design.md) | AstraEMU BGI core 的进程内模块和 IPC 输出。 |
| [game-observations.md](game-observations.md) | 三个本地游戏目录的 archive 与 payload 观测结果。 |
| [tooling.md](tooling.md) | 安全 probe、参考工具和报告字段。 |
| [implementation-checklist.md](implementation-checklist.md) | 实现顺序、验收项和当前风险。 |

## 证据范围

实现事实来自以下本地只读参考：

- `D:\Workspace\ethornell-rs`
- `D:\Workspace\BGITool`
- `D:\Workspace\FuckGalEngine\BGI`
- `E:\Games\樱之诗春之雪`
- `E:\Games\サクラノ詩`
- `E:\Games\素晴らしき日々15th`

游戏目录只用于统计 archive header、entry metadata、payload magic、局部 opcode 和 header 字段。文档不复制完整商业 script、图像、音频或影片内容，也不记录可用于绕过商业保护或访问控制的步骤。后续 machine-readable report 应使用 case id、相对 archive path 和 hash 摘要，不写入开发机私有绝对路径。

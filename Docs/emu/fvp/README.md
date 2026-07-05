# AstraEMU FVP

本目录只记录 AstraEMU 的 FVP family 设计输入和实施口径。资料来自 `D:/Workspace/rfvp` 的实现、工具文档，以及本地合法样本「樱花萌放」的文件级观察。这里不保存商业 payload、剧情文本、截图或任何绕过访问控制的步骤。

## 阅读顺序

| 页面 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | rfvp 代码入口、工具入口和样本文件清单 |
| [archive-format.md](archive-format.md) | FVP `.bin` archive 结构、VFS 映射和样本统计 |
| [script-format.md](script-format.md) | `.hcb` header、opcode、syscall table 和编码规则 |
| [script-execution.md](script-execution.md) | VM context、thread state、syscall dispatch 和 yield 流程 |
| [presentation-and-media.md](presentation-and-media.md) | graph/text/prim/audio/movie 的表现层映射 |
| [runtime-core-design.md](runtime-core-design.md) | AstraEMU FVP compat core 边界和 IPC 输出 |
| [game-observations.md](game-observations.md) | 「樱花萌放」样本观察，保留 metadata 和 hash |
| [tooling.md](tooling.md) | disassembler、assembler、hcb2lua、lua2hcb、nvsg_pack 的使用边界 |
| [implementation-checklist.md](implementation-checklist.md) | FVP family adapter 最小实施清单和验收口径 |

## 范围

FVP 在 AstraEMU 中是独立 family adapter。Compat core 持有 `.hcb` VM、`.bin` VFS、legacy syscall mapper、媒体解析状态和 save snapshot。Manager 只接收本地结构化 trace、`PresentationCommand`、`AudioCommand`、`TextCaptureEvent`、`StateMachineTrace` 和 media block reference。

FVP 不改变 EngineCore 的 Actor/Component + StateMachine 权威模型，也不把 rfvp 的单 family 主循环、no_std 约束或平台 host 细节变成公共 Runtime contract。

## 样本基线

「樱花萌放」样本可作为本地 case report 输入。记录时只保留：

- 文件名、大小、hash prefix、archive entry 数量和 media magic。
- `.hcb` header metadata、syscall 名称和 opcode offset。
- 本地结构化 VM trace，例如 `pc`、opcode、syscall name、参数类型和返回类型。

不能保留：

- 剧情正文、完整反编译脚本、素材内容、截图、音频或视频帧。
- 游戏可执行文件、补丁安装包、第三方 DLL 或任何修改商业 payload 的说明。

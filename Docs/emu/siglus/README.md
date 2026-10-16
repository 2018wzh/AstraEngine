# AstraEMU Siglus

本目录记录 AstraEMU 接入 Siglus family 所需的公开契约、格式事实和验收口径。内容来自 Siglus Rust 参考实现、历史 C/C++ 取证代码，以及两个合法本地样本的只读观测。

这些文档只覆盖 family plugin 的合法读取、执行和诊断边界。不包含商业 payload、脚本文本全文、截图、音频、视频、key 表、key 提取流程、补丁注入步骤或访问控制规避说明。

## 文档索引

| 文件 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 事实来源、已读文件和样本观测范围 |
| [archive-format.md](archive-format.md) | 目录布局、`Scene.pck`、资源文件和 pack 类容器 |
| [scene-pck-ss.md](scene-pck-ss.md) | `Scene.pck` 到 `.ss` 场景块的结构映射 |
| [script-format.md](script-format.md) | `.ss` header、字符串表、label、`CD_*` bytecode |
| [script-execution.md](script-execution.md) | VM 栈、调用、命令分发、等待边界 |
| [presentation-and-media.md](presentation-and-media.md) | G00、OMV、OVK、OWP、NWA、Ogg、MPEG/WMV 的运行时口径 |
| [gameexe-config.md](gameexe-config.md) | `Gameexe.dat`/`Gameexe.chs` 解析和配置表契约 |
| [MODIFICATIONS](../../../Emulator/Source/Families/astra-emu-siglus/MODIFICATIONS.md) | 当前核心基线与必要嵌入差异 |
| [game-observations.md](game-observations.md) | anemoi 体験版与 Rewrite_PLUS 的本地结构化观测 |
| [tooling.md](tooling.md) | 现有 probe 工具和安全使用边界 |
| [实施计划](../../status/implementation-plan.md) | 当前整改顺序与验收条件 |

## 接入原则

Siglus Family 复用原 VM、renderer、Kira、实时时钟和保存实现。Manager 通过 [Family API](../../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md) 获取不可变 CPU lease、有界 PCM 和 typed 请求；Family 不进入 RuntimeWorld 或 AstraVN。

当前绘制 generation 用于跳过无新输出的回读；变化帧的同步等待、分配和受阻关闭仍需收敛，详见[性能审计](../../status/performance-upstream-audit-2026-10-12.md)。

## 验收

当前已运行 GPU 标题/媒体、PCM、原生全局保存和冷重开。新基线菜单/槽位/代表流程、设备听音、GPU 警告归因与性能对照仍待验收。格式研究不替代实际产品运行。

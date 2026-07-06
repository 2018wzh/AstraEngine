# AstraEMU Siglus

本目录记录 AstraEMU 接入 Siglus family 所需的公开契约、格式事实和验收口径。内容来自 Siglus Rust 参考实现、历史 C/C++ 取证代码，以及两个合法本地样本的只读观测。

这些文档只覆盖兼容 core 的合法读取、执行和诊断边界。不包含商业 payload、脚本文本全文、截图、音频、视频、key 表、key 提取流程、补丁注入步骤或访问控制规避说明。

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
| [runtime-core-design.md](runtime-core-design.md) | AstraEMU Siglus compat core 设计 |
| [game-observations.md](game-observations.md) | anemoi 体験版与 Rewrite_PLUS 的本地结构化观测 |
| [tooling.md](tooling.md) | 现有 probe 工具和安全使用边界 |
| [implementation-checklist.md](implementation-checklist.md) | 按功能域拆分的实现清单 |

## 接入原则

Siglus 在 AstraEMU 中是独立 compat core，不进入 AstraVN canonical story，也不把 Siglus 的对象模型上提为 EngineCore API。Manager 只接收 `PresentationCommand`、`AudioCommand`、`TextCaptureEvent`、`StateMachineTrace`、snapshot reference 和 diagnostics。

Runtime 权威状态留在 Siglus core 内。跨进程边界只传递结构化事件、本地结构化 trace 和可复现的本地资源引用；Manager 不解析 core 私有 VM 内存。

## 最小验收

第一阶段只要求合法安装目录的只读启动和 trace：识别 `Scene.pck`、读取 `Gameexe.*` header、枚举媒体格式、构建本地结构化 report。后续再接脚本执行、presentation 和 save/load。

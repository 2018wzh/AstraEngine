# Minori AstraEMU 资料入口

Minori family 资料面向 `夏空のペルセウス` 这类 PAZ + `.sc` 脚本游戏。目标是让 AstraEMU compat core 能完成本地合法样本的资源定位、脚本反编译、演出命令建模和运行时状态复现。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 参考目录、游戏样本和可用工具 |
| [archive-format.md](archive-format.md) | PAZ、MYS、补丁包和 key 外置规则 |
| [paz.md](paz.md) | PAZ TOC、压缩、key 输入和诊断细节 |
| [script-format.md](script-format.md) | `.sc`、`.mys`、`.acr` 的脚本/文本关系 |
| [sc-script.md](sc-script.md) | `.sc` 指令流、message/select 观测和反编译形态 |
| [script-execution.md](script-execution.md) | VM tick、跳转、等待、选择和 save snapshot |
| [presentation-and-media.md](presentation-and-media.md) | 立绘、背景、音频、movie 和窗口系统 |
| [runtime-core-design.md](runtime-core-design.md) | AstraEMU Minori core 的模块拆分 |
| [game-observations.md](game-observations.md) | `夏空のペルセウス` 本地样本事实 |
| [tooling.md](tooling.md) | `Tools/AstraEMU/minori_*.py` 用法 |
| [implementation-checklist.md](implementation-checklist.md) | 可编码验收清单 |

## 边界

PAZ key、exe patch、安装器保护和 hook 资料不进入公共实现。AstraEMU 只提供外部 key 输入、只读 archive reader、反编译诊断和 family core 状态机。

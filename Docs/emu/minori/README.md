# AstraEMU Minori

Minori family 资料面向 PAZ + `.sc` 脚本游戏。本阶段已有 `astra-emu-minori`、公共 VFS/support 层、通用 `astra-emu-cli vfs --family minori` 和独立研究工具 `astra-emu-minori-cli`；尚未实现 Minori VM、演出执行、存档和完整游戏模拟。

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
| [runtime-family-plugin.md](runtime-family-plugin.md) | AstraEMU Minori provider session 的模块拆分 |
| [game-observations.md](game-observations.md) | `夏空のペルセウス` 本地样本事实 |
| [tooling.md](tooling.md) | 通用 VFS CLI、Minori 私有导入与研究工具 |
| [implementation-checklist.md](implementation-checklist.md) | 可编码验收清单 |

## 边界

PAZ key、exe patch、安装器保护和 hook 资料不进入公共实现。`astraemu.patch.luau`、明文 cache、导出资源、脚本文本和 disassembly 都是本地私有数据，不进入 Git、package、report 或日志。

`scr/st/sys/se/voice/mov.paz` 必须同时存在且非空。当前授权样本满足这一条件。manifest v2 全量 verify 覆盖 6 个 source、9837 个 entry、29720 次 range read 和 2403596354 个 decoded bytes；同 identity 第二轮全部命中 cache，聚合 hash 保持一致。89 个脚本的 payload-free census 也已通过。Linux FUSE 与 macOS 验收仍需独立证据，不能由 Windows VFS 结果替代。

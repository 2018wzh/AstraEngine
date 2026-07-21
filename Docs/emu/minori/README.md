# AstraEMU Minori

Minori family 资料面向 PAZ + `.sc` 脚本游戏。本阶段已有 `astra-emu-minori`、公共 VFS/support 层、通用 `astra-emu-cli vfs --family minori` 和独立研究工具 `astra-emu-minori-cli`。runtime 已具备 typed IR、已确认 control-flow、消息、音频与无 stand stage 子集、可序列化状态和签名动态 provider 的 Headless E2 slice；真实八包已跑到竖排标题。选项、普通 voice、人物站位、transition 动画、系统页和完整路线仍未实现。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 参考目录、游戏样本和可用工具 |
| [research-sources.md](research-sources.md) | 资料来源、revision、许可证和适用边界 |
| [porting-log.md](porting-log.md) | 按日期维护的移植事实、测试和 blocker |
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

当前授权样本包含 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive，其中 `bg.paz` 另有 A–J 十个连续分卷，合计 18 个物理文件。八包 manifest v2 full verify 已覆盖 14502 个 entry、43818 次 range read 和 6624958365 个 decoded bytes，验证时显式关闭明文 cache。89 个脚本的 payload-free census 已通过。cache identity 复核因平台缓存卷空间不足仍是 blocker；Linux FUSE 与 macOS 验收也需独立证据，不能由 Windows VFS 结果替代。

# AstraEMU Minori

Minori family 资料面向 PAZ + `.sc` 脚本游戏。当前已有 `astra-emu-minori`、公共 VFS/support 层、通用 `astra-emu-cli vfs --family minori` 和独立研究工具 `astra-emu-minori-cli`。runtime 已接入 typed IR、已确认控制流、消息、选择、stage/character、音频、影片、系统页、save/restore 和可序列化状态；签名动态 provider 已用真实八包跑完首条路线并返回标题。`.char trans/.char vis` 的动态立绘合同已有合成 Headless GPU 视觉证据，但真实脚本没有这两类命令。同一 clean Release identity 的三次十分钟 120 Hz GPU E2 已通过；完整鉴赏、正式音频听审和 Windows E3 仍未完成。Config 已有行为级 Headless E2，但还没有正式人工音频或 Windows E3 证据。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 参考目录、游戏样本和可用工具 |
| [research-sources.md](research-sources.md) | 资料来源、revision、许可证和适用边界 |
| [porting-log.md](porting-log.md) | 按日期维护的移植事实、测试和 blocker |
| [save-load-e2.md](save-load-e2.md) | ABI v9 writable-file save/load 的 Headless E2 证据 |
| [archive-format.md](archive-format.md) | PAZ、MYS、补丁包和 key 外置规则 |
| [paz.md](paz.md) | PAZ TOC、压缩、key 输入和诊断细节 |
| [script-format.md](script-format.md) | `.sc`、`.mys`、`.acr` 的脚本/文本关系 |
| [sc-script.md](sc-script.md) | `.sc` 指令流、message/select 观测和反编译形态 |
| [script-execution.md](script-execution.md) | VM tick、跳转、等待、选择和 save snapshot |
| [presentation-and-media.md](presentation-and-media.md) | 立绘、背景、音频、movie 和窗口系统 |
| [runtime-family-plugin.md](runtime-family-plugin.md) | AstraEMU Minori provider session 的模块拆分 |
| [game-observations.md](game-observations.md) | `夏空のペルセウス` 本地样本事实 |
| [tooling.md](tooling.md) | 通用 VFS CLI、key 文件和研究工具 |
| [implementation-checklist.md](implementation-checklist.md) | 可编码验收清单 |

## 边界

Minori mount 只接受游戏目录内由 launch profile 指定的安全相对 `key_file`。`key.toml` 使用严格 schema `astra.emu.minori.keys.v1`，mount 时读取一次；VFS 随后从加密 source 流式解密，不建立明文 cache 或临时文件。旧 `astraemu.patch.luau`、private profile、GARbro importer 和 cache 字段都已硬删除。

PAZ key、exe patch、安装器保护和 hook 资料不进入公共实现。`key.toml`、导出资源、脚本文本和 disassembly 都是本地私有数据，不进入 Git、package、report 或日志。

当前授权样本包含 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive，其中 `bg.paz` 另有 A–J 十个连续分卷，合计 18 个物理文件。迁移前的 no-cache manifest v2 full verify 覆盖 14,502 个 entry、43,818 次 range read 和 6,624,958,365 个 decoded bytes。新的 key-file/streaming identity 仍需重跑真实 full verify；89 个脚本的 payload-free census 已通过。Linux FUSE 与 macOS 验收仍需独立证据，不能由 Windows VFS 结果替代。

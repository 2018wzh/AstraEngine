# AstraEMU FVP

本目录只记录 AstraEMU 的 FVP family 设计输入和实施口径。资料来自 FVP 研究实现、工具文档，以及本地合法样本的文件级观察。这里不保存商业 payload、剧情文本、截图或任何绕过访问控制的步骤。

## 阅读顺序

| 页面 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | rfvp 代码入口、工具入口和样本文件清单 |
| [archive-format.md](archive-format.md) | FVP `.bin` archive 结构、VFS 映射和样本统计 |
| [script-format.md](script-format.md) | `.hcb` header、opcode、syscall table 和编码规则 |
| [script-execution.md](script-execution.md) | VM context、thread state、syscall dispatch 和 yield 流程 |
| [presentation-and-media.md](presentation-and-media.md) | graph/text/prim/audio/movie 的表现层映射 |
| [runtime-family-plugin.md](runtime-family-plugin.md) | AstraEMU FVP family plugin 的 session 边界和 step 输出 |
| [thin-fork.md](thin-fork.md) | 已取代的 Family ABI v9 fork 边界记录 |
| [rfvp-fork-audit.md](rfvp-fork-audit.md) | pinned RFVP fork 的职责审计和当前阻断项 |
| [game-observations.md](game-observations.md) | 「樱花萌放」样本观察，保留 metadata 和 hash |
| [tooling.md](tooling.md) | disassembler、assembler、hcb2lua、lua2hcb、nvsg_pack 的使用边界 |
| [implementation-checklist.md](implementation-checklist.md) | FVP family adapter 实施清单和验收口径 |

## 范围

本轮 [独立 Host 重构](../../migrations/astraemu-independent-host.md) 要求 FVP 直接实现独立 Family ABI：接收 elapsed 与物理输入，自行持有原生文件、VM、解码、混音、字体、绘制与存档。Host 同步复制 Family 借出的最终 CPU 帧；独立音频 worker 向 Host 提交混合 PCM。不再使用 Layer2D、Hook、Host writable surface/VFS 或统一 snapshot/save。

适配源固定为 RFVP revision `f4f64a5bb726c1759350a666a35e0a454b810f61`，旧接口审阅见 [RFVP fork audit](rfvp-fork-audit.md)。独立 adapter 正在实现，当前进度与待测游戏流程见 [Stage 5](../../status/stages/stage-5-astra-emu.md)。

FVP 不改变 EngineCore 的运行模型，也不把单 Family 主循环和平台细节变成公共 Runtime contract。本轮 FVP 不声明文本替换 capability，不修改其翻译路径；游戏原生存档与系统页继续由 RFVP 持有。

## 样本基线

合法本地样本可作为 local case report 输入。记录时只保留：

- 文件名、大小、hash prefix、archive entry 数量和 media magic。
- `.hcb` header metadata、syscall 名称和 opcode offset。
- Evidence profile 下的本地 crash trace 摘要，例如 `pc`、opcode offset 和稳定诊断码；Shipping profile 不记录逐 opcode trace。

不能保留：

- 剧情正文、完整反编译脚本、素材内容、截图、音频或视频帧。
- 游戏可执行文件、补丁安装包、第三方 DLL 或任何修改商业 payload 的说明。

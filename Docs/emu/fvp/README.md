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
| [thin-fork.md](thin-fork.md) | Family ABI v9 的 RFVP fork、Host surface 与薄 adapter 边界 |
| [rfvp-fork-audit.md](rfvp-fork-audit.md) | pinned RFVP fork 的职责审计和当前阻断项 |
| [game-observations.md](game-observations.md) | 「樱花萌放」样本观察，保留 metadata 和 hash |
| [tooling.md](tooling.md) | disassembler、assembler、hcb2lua、lua2hcb、nvsg_pack 的使用边界 |
| [implementation-checklist.md](implementation-checklist.md) | FVP family adapter 实施清单和验收口径 |

## 范围

FVP 在 AstraEMU 中是 `Ported + SingleLayer` 的 engine-native family plugin。RFVP fork 直接实现 Family ABI v9 provider：它消费 Host input/wait/audio/control DTO，在 Hook 后取得唯一 writable surface lease，直接光栅化并提交 `Unchanged`、`Full` 或像素坐标 `Rects` damage；存档只通过 per-game writable-file Host port。AstraEngine 内的 `astra-emu-fvp` 只保留 dylib root export、build identity、descriptor、panic containment 和最终错误边界，不转换 scene、texture、text 或 save DTO。

当前 pinned fork revision 为 `f4f64a5bb726c1759350a666a35e0a454b810f61`。该 revision 的 Astra adapter-facing 结构已符合上述边界，但 fork 内仍有旧 hosted semantic-delta、snapshot/restore 和策略-limit 实现；详见 [RFVP fork audit](rfvp-fork-audit.md)。在这些旧路径移除并由 fork 仓库形成单一审查提交前，FVP v9 release gate 保持 blocking。

FVP 不改变 EngineCore 的 Actor/Component + StateMachine 权威模型，也不把 rfvp 的单 family 主循环、no_std 约束或平台 host 细节变成公共 Runtime contract。Family core 自己完成字体 fallback、shaping、换行和绘制；Host 不提供文本 overlay、翻译 cache 或 save-slot 语义。

## 样本基线

合法本地样本可作为 local case report 输入。记录时只保留：

- 文件名、大小、hash prefix、archive entry 数量和 media magic。
- `.hcb` header metadata、syscall 名称和 opcode offset。
- Evidence profile 下的本地 crash trace 摘要，例如 `pc`、opcode offset 和稳定诊断码；Shipping profile 不记录逐 opcode trace。

不能保留：

- 剧情正文、完整反编译脚本、素材内容、截图、音频或视频帧。
- 游戏可执行文件、补丁安装包、第三方 DLL 或任何修改商业 payload 的说明。

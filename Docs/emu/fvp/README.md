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
| [rfvp-fork-audit.md](rfvp-fork-audit.md) | 历史 Family ABI v9 fork 审计，不作当前验收依据 |
| [game-observations.md](game-observations.md) | 「樱花萌放」样本观察，保留 metadata 和 hash |
| [tooling.md](tooling.md) | disassembler、assembler、hcb2lua、lua2hcb、nvsg_pack 的使用边界 |
| [implementation-checklist.md](implementation-checklist.md) | FVP family adapter 实施清单和验收口径 |

## 范围

FVP 直接实现 [Family API v13](../../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)：接收 elapsed 与物理输入，自持原生文件、VM、解码、混音、字体、绘制与存档。Host 持有不可变最终 CPU frame lease，使用 epoch/generation 区分 Ready/Pending/Unchanged；独立音频 worker 提交有界 PCM。不使用 Layer2D、Host writable surface/VFS 或统一 snapshot/save。

按用户要求，当前 RFVP 0.6.0 直接基于上游 HEAD `af557db750c7833274f22a6d82b0dc33cdea2f85`，仅增加一个必要适配提交，并以完整 submodule 集成。精确适配提交、差异分类与未发布状态以 [MODIFICATIONS.md](../../../Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md) 为准；原发布/来源历史保留，不维护长期兼容双轨。

保留原 GPU primitive/mask、原 Movie 解码/合成/音轨与原生保存。Family 仅收集音频命令与元数据变化，不再有第二 WMV 播放器；元数据观察不能授权像素回收，必须由原生 GPU cache 确认 generation 已上传。实际游戏发现及修复、检查和未验收范围见[性能审计](../../status/performance-upstream-audit-2026-10-12.md#实际游戏暴露的像素所有权缺口与修复)和[实施状态](../../status/implementation-plan.md)。

FVP 不改变 EngineCore 的运行模型，也不把单 Family 主循环和平台细节变成公共 Runtime contract。本轮 FVP 不声明文本替换 capability，不修改其翻译路径；游戏原生存档与系统页继续由 RFVP 持有。

## 样本基线

合法本地样本可作为 local case report 输入。记录时只保留：

- 文件名、大小、hash prefix、archive entry 数量和 media magic。
- `.hcb` header metadata、syscall 名称和 opcode offset。
- 稳定诊断码、有限错误上下文及原生工具输出的 `pc`/opcode offset；不额外维护 Evidence 事务协议或逐指令 trace ring。

不能保留：

- 剧情正文、完整反编译脚本、素材内容、截图、音频或视频帧。
- 游戏可执行文件、补丁安装包、第三方 DLL 或任何修改商业 payload 的说明。

全局设置、已读状态和系统变量使用原 `GlobalSaveDataV1`/RFVG，普通槽使用原 `SaveStateSnapshotV1`/RFVS；直接沿用上游读写流程与校验，不新增旧文件识别、迁移、兼容 reader 或专门报错。启动在首个 VM tick 前加载全局状态，关闭通过 Family 原子文件接口保存；读写错误正常传播。

删除旧 V2 演出容器扩展和重复 hosted codec。当前保存仅复用 V1 已有字段，在保存边界捕获修改过的图像，读档恢复已有像素及几何；不为此逐帧克隆或序列化。保留必要 session globals、字体注入与 GPU/音频/关闭边界，不复制原 VM、renderer 或保存格式。

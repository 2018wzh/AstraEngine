# AstraEMU FVP

FVP Family 复用 RFVP 原 VM、GPU/mask、字体、视频、混音和原生保存。跨 Host 交付不可变 CPU frame lease 与有界 PCM；Family 收集绘制元数据用于变化检测，像素回收由原 GPU cache 授权。

## 当前入口

- [Family API](../../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)：配置、帧、音频、窗口和生命周期。
- [MODIFICATIONS](../../../Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md)：精确上游/适配提交及差异分类。
- [性能审计](../../status/performance-upstream-audit-2026-10-12.md)：当前问题与运行证据；[实施计划](../../status/implementation-plan.md)：整改顺序。

当前 HEAD＋单提交结构通过，严格最小适配尚未验收；优先修复原 no_std 输入提交回归并分离原生功能修复。保存沿用上游 V1/RFVS 和 GlobalSaveDataV1/RFVG，旧 V2 扩展/重复 codec 已删除。本 Family 不声明文本替换 capability。

## 格式与工具资料

| 页面 | 内容 |
| --- | --- |
| [源码索引](source-inventory.md) | 原 RFVP 模块与样本文件 metadata |
| [归档格式](archive-format.md) | BIN、VFS 与资源边界 |
| [脚本格式](script-format.md) | HCB header、opcode、syscall 和编码 |
| [脚本执行](script-execution.md) | context、thread、dispatch 与 yield |
| [呈现与媒体](presentation-and-media.md) | 原生 graph/text/prim/audio/movie 职责 |
| [样本观察](game-observations.md) | 《樱花萌放》文件级事实 |
| [工具](tooling.md) | disassembler、assembler、HCB/Lua 与 NVSG 工具 |

格式研究不定义另一套执行或 Host 输出协议。商业正文、资源、截图和存档留在 ignored 工作区；公开记录只含必要 metadata、诊断和自制复现。

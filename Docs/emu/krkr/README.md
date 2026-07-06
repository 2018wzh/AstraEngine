# AstraEMU KrKr 兼容文档

本目录只记录 AstraEMU 对 KrKr/KAG/TJS family 的兼容边界、输入格式和验收口径。当前内容是文档规格，不表示 AstraEMU 已经实现 KrKr runtime。

## 范围

KrKr family 覆盖 KiriKiri2/KAG/TJS 常见游戏形态：XP3 archive、KAG `.ks`、TJS source/bytecode、PSB 形态的 `.ks.scn`、TLG/PNG/JPG/UI 资源、BGM/voice/movie 和插件扩展。AstraEMU 的目标是让 family plugin 复现旧引擎行为，再把 trace、media ref、TextCaptureEvent 和 snapshot section 交给 RuntimeWorld。EngineCore 不接收 TJS VM、旧插件对象或原生 GPU/audio handle。

本目录使用两个本地证据源：一个 KrKr 商业样本的本地结构化结构信息，以及 `FuckGalEngine/Krkr` 中的 XP3、PSB、KAG 文本处理和 TJS 调试材料。文档只写文件名、格式结构、计数、hash/flag 类元数据和自造示例；不复制商业脚本、图像、音频、视频或绕过流程。

## 页面

| 页面 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 样本和参考代码的证据范围 |
| [archive-format.md](archive-format.md) | XP3 header、index、chunk、segment 和校验字段 |
| [xp3-layering.md](xp3-layering.md) | base/patch archive 的虚拟 storage 覆盖规则 |
| [script-format.md](script-format.md) | TJS、KAG `.ks`、`.ks.scn`、PSB 和辅助文本格式 |
| [kag-tjs.md](kag-tjs.md) | KAG/TJS boot、`SystemConfig`、`KAGLoadScript` 和 tag conductor |
| [script-execution.md](script-execution.md) | 脚本执行、等待、输入、save/load 和 trace 语义 |
| [presentation-and-media.md](presentation-and-media.md) | layer、transition、text、audio、movie 和插件媒体能力 |
| [runtime-core-design.md](runtime-core-design.md) | AstraEMU KrKr family plugin 的最小模块划分 |
| [game-observations.md](game-observations.md) | 3lj 样本的本地结构化结构观察 |
| [tooling.md](tooling.md) | 只读 probe、index diff、trace 和媒体 smoke 工具需求 |
| [implementation-checklist.md](implementation-checklist.md) | 实现顺序和 release gate 检查项 |

## 与共享契约的关系

- Family plugin 服从 [AstraEMU Family Plugin Contract](../../contracts/astraemu-ipc.md)。
- 媒体输出服从 [Media Contract](../../contracts/media.md)。
- Runtime 事件进入 [Runtime Contract](../../contracts/runtime.md)。
- KrKr 不能把 KAG/TJS 对象模型反向推入 [AstraEMU Module](../../modules/astra-emu.md) 或 EngineCore。

## 最小验收

KrKr family 的第一阶段只要求读出 XP3 index、建立虚拟 storage、识别 boot 脚本、跑通一个本地结构化 headless trace，并把不支持的插件、字节码、PSB 场景和媒体能力明确记录为 diagnostics。任何需要商业 payload 的验证都留在本地 report，不进入仓库。

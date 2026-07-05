# AstraEMU Module

AstraEMU 是旧 VN 模拟器和现代化套件。它复用 Astra Runtime、Media、Script、Plugin、Release Gate，但不进入 NativeVN 创作链路。

## Manager/Core 架构

Manager 负责窗口、输入、配置、core 选择、provider selection、插件分发、报告和 overlay。Compat core 是独立进程，持有 family 状态机、私有 VM、资源 resolver 和 legacy API mapper。

IPC 使用 framed local RPC + shared memory。Core 输出 RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent、StateMachineTrace 和 snapshot reference。

## Family 路线

| 顺序 | Family | 参考 |
| --- | --- | --- |
| 1 | Artemis | system script、tag executor、现代商业 VN case；v1 可用 family |
| 2 | KrKr/KAG/TJS | 常见 XP3/KAG/TJS 生态 |
| 3 | BGI/Ethornell | [ethornell-rs](https://github.com/xmoezzz/ethornell-rs) |
| 4 | SoftPAL | `D:/Workspace/sena-rs` |
| 5 | FVP | `D:/Workspace/rfvp` |
| 6 | Siglus | `D:/Workspace/siglus_rs` |
| 7 | Minori | `D:/Workspace/FuckGalEngine/Minori` |

每个 family 的实现级调研、格式说明、脚本演出拆解和工具命令放在 [../emu/README.md](../emu/README.md)。

## Luau Patch / Decode

EMU 提供 capability sandbox、read-only mount、archive reader、index mapper、decode helper、diagnostics 和 report API。用户 Luau 描述补丁和解码流程；EMU 不内置商业绕过逻辑。

## 验收

每个 family 必须产出 local case report，并通过 full-flow YAML scenario：boot、main route、choice、text、voice、BGM、SE、movie、system menu、config、save/load、backlog、replay 和 shutdown。报告可包含本地路径、hash、offset、entry count 和短摘录，但不能提交完整商业 payload、图片、音频、视频、完整剧情脚本或保护绕过材料。

Artemis v1 core 的进程边界、probe、legacy Lua bridge、snapshot 和 report policy 见 [AstraEMU Artemis Core Blueprint](../implementation/astraemu-artemis-core.md)。

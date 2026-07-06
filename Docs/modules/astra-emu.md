# AstraEMU Module

AstraEMU 是旧 VN 模拟器和现代化套件。它复用 Astra Runtime、Media、Script、Plugin、Release Gate，但不进入 NativeVN 创作链路。

## Engine-native 架构

Manager 负责窗口、输入、配置、family 选择、provider selection、插件分发、报告和 overlay。Manager 创建并驱动 AstraEngine `RuntimeWorld`，legacy family 以 in-process plugin/provider 接入。

Family plugin 注册 `LegacyRuntimeProvider` facade。Provider 通过 session 持有 archive resolver、旧 VM、media state、snapshot serializer 和 diagnostics，AstraEngine StateMachine 只调用粗粒度 legacy lifecycle action。旧引擎语义必须落成 RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent、StateMachineTrace、AwaitToken 和 save section；插件不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

`EMUCoreBridge` 只作为 extension point 保留，用于受限实验或外部工具桥接，不是 v1 主架构。

## Family 路线

| 顺序 | Family | 参考 |
| --- | --- | --- |
| 1 | Artemis | system script、tag executor、现代商业 VN case；v1 可用 family |
| 2 | KrKr/KAG/TJS | 常见 XP3/KAG/TJS 生态 |
| 3 | BGI/Ethornell | BURIKO/DSC/BCS/BP 生态和公开参考实现 |
| 4 | SoftPAL | PAC/DAT、extcall 和传统脚本 VM 研究 |
| 5 | FVP | HCB VM、pack/media resolver 和 syscall mapper 研究 |
| 6 | Siglus | Scene.pck、Gameexe、`.ss`、G00/media 研究 |
| 7 | Minori | PAZ + `.sc` 脚本研究 |

每个 family 的实现级调研、格式说明、脚本演出拆解和工具命令放在 [../emu/README.md](../emu/README.md)。研究页可以保留旧引擎原始术语；产品 contract 以本页和 [AstraEMU Legacy Runtime Provider Contract](../contracts/astraemu-ipc.md) 为准。

## Luau Patch / Decode

EMU 提供 capability sandbox、read-only mount、archive reader、index mapper、decode helper、diagnostics 和 report API。用户 Luau 描述补丁和解码流程；EMU 不内置商业绕过逻辑。

## 验收

每个 family 必须产出 local case report，并通过 full-flow YAML scenario：boot、main route、choice、text、voice、BGM、SE、movie、system menu、config、save/load、backlog、replay 和 shutdown。报告只包含 hash、offset、entry count、coverage、diagnostics 和脱敏 metadata，不能提交完整商业 payload、图片、音频、视频、完整剧情脚本、私有绝对路径或保护绕过材料。

Legacy Runtime Framework 的 session、step、effect 和 snapshot 设计见 [AstraEMU Legacy Runtime Framework](../implementation/astraemu-legacy-runtime-framework.md)。Artemis v1 family plugin 的 probe、legacy Lua bridge、snapshot 和 report policy 见 [AstraEMU Artemis Core Blueprint](../implementation/astraemu-artemis-core.md)。

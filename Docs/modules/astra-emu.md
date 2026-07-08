# AstraEMU Module

AstraEMU 是旧 VN 模拟器和现代化套件。它复用 Astra Runtime、Media、Script、Plugin、Asset/VFS、Game Runtime Provider 和 Release Gate，但不进入 NativeVN 创作链路。

## Engine-native 架构

Manager 负责窗口、输入、配置、family 选择、provider selection、插件分发、报告、overlay、文本管线和滤镜 preset。Manager 自身是 Program target；被启动的 legacy case 通过 `AstraEmuRuntimeProvider` 作为 gameplay runtime session 运行。Provider 创建并驱动 AstraEngine `RuntimeWorld`，legacy family 以 in-process plugin/provider 接入。

Family plugin 注册 `LegacyRuntimeProvider` facade。Provider 通过 session 持有 archive resolver、旧 VM、media state、snapshot serializer 和 diagnostics，AstraEngine StateMachine 只调用 `AstraEmuRuntimeProvider` 暴露的 runtime step action。旧引擎语义必须落成 RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent、StateMachineTrace、AwaitToken 和 save section；插件不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

Family 内部可以把 VM 映射为私有 scheduler、context、basic-block 和 action 状态机。多线程或多 context legacy VM 使用多个 child state machine，由 deterministic scheduler 按固定 `(priority, context_id, sequence)` 推进。公共 Runtime 只看到有序 effect、await、trace、snapshot hash 和 diagnostic。

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

实现顺序仍以 Artemis 作为 v1 可用 family；自动探测的默认用户体验按格式普及度排序：KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori。用户 profile 可以显式覆盖自动选择。

每个 family 的实现级调研、格式说明、脚本演出拆解和工具命令放在 [../emu/README.md](../emu/README.md)。研究页可以保留旧引擎原始术语；产品 contract 以本页和 [AstraEMU Legacy Runtime Provider Contract](../contracts/astraemu-ipc.md) 为准。

## Luau Patch / Decode

EMU 用户脚本统一使用 Luau。Trusted Project Profile 可以开启 read-only VFS mount、patch overlay、decode transform、text/media hook、VM trace、diagnostic 和 deterministic effect intent。状态注入只能变成 `LegacyEffect`、Blackboard、input 或 tag intent，在 fixed tick 边界进入 Runtime。脚本请求未授权 key 提取、商业保护处理、访问控制规避、raw filesystem/network/system call 或 native handle 时，Manager 隔离禁用该脚本，并按无补丁模式继续 case。

## Text / Translation / Filter

`TextCaptureEvent` 进入 Manager 的 `TextCapturePipeline`。默认 report 只写 hash、长度、source ref 和 speaker metadata；用户本地 opt-in 后才能保存全文 dump。翻译通过 `TranslationProvider` slot 接入，DeepL-style provider 走 batch fallback，LLM provider 可以通过 MCP session streaming 更新 overlay。翻译 overlay 非权威，不进入 replay hash；术语表和角色上下文读取 Stage 4 runtime memory 的授权 namespace。

滤镜复用 Media `FilterGraph`。AstraEMU profile 可以绑定 final-frame preset 和 per-layer preset；per-layer 只依赖 `PresentationCommand` 的 layer id 或 role。family 不提供 layer metadata 时，只启用 final-frame 并输出 diagnostic。不新增 family 专属 shader/filter API。

## 验收

每个 family 必须产出 local case report，并通过 full-flow YAML scenario：boot、main route、choice、text、voice、BGM、SE、movie、system menu、config、save/load、backlog、replay 和 shutdown。报告只包含 hash、offset、entry count、coverage、diagnostics 和脱敏 metadata，不能提交完整商业 payload、图片、音频、视频、完整剧情脚本、私有绝对路径或保护绕过材料。

Legacy Runtime Framework 的 session、step、effect 和 snapshot 设计见 [AstraEMU Legacy Runtime Framework](../implementation/astraemu-legacy-runtime-framework.md)。VM 到状态机的映射见 [EmulatorCore StateMachine Mapping](../implementation/emulator-core-state-machine.md)。Artemis v1 family plugin 的 probe、legacy Lua bridge、snapshot 和 report policy 见 [AstraEMU Artemis Core Blueprint](../implementation/astraemu-artemis-core.md)。

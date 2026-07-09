# AstraRPG Module

AstraRPG 是后续 RPG gameplay runtime 模块。它通过 `AstraRpgRuntimeProvider` 与 EngineCore 对接，不把 RPG、AI agent 或 TRPG 规则塞进 Runtime/Core。TRPG 能力作为 AstraRPG 的 `rpg.trpg` profile/ruleset layer 存在，不建立独立顶层模块，也不注册独立 runtime provider。

## Responsibilities

- 管理 `RpgSession`、ruleset/profile、RPG actor sheet、party、inventory、quest、encounter、battle、faction、relationship 和 memory ledger。
- 接收 player/script/runtime-ai/GM/system 产生的 `RpgIntent`，通过 IntentValidator 转为 `RpgEffect` 或 blocking diagnostic。
- 把 `RpgEffect` 转为 `ActionEffect`、runtime event、await token、presentation/audio command 或 provider save section。
- 使用通用 `astra-policy` Luau sandbox 执行规则策略，所有写入必须经 host API 变成可序列化 effect。
- 固化 AI proposal 为 `CommittedAgentOutput`，保证 save/load/replay 不重新请求 provider。
- 为 Editor 提供 Map、Quest、Party、Inventory、Encounter、Behavior Graph、RPG Inspector 和 TRPG sheet/transcript 的 metadata，不传递 widget 或 runtime 内部对象。

## TRPG Profile

`rpg.trpg` profile 负责 character sheet、deterministic dice、check resolver、ruling、transcript、seat authority 和 privacy/redaction。它复用 AstraRPG session、policy bundle、save/package container 和 release gate。

TRPG profile 不能：

- 独立创建 `AstraTrpgRuntimeProvider`；
- 使用 `trpg.*` 顶层 package/save namespace；
- 绕过 `RpgIntent`、`RpgEffect`、seat authority 或 transcript redaction；
- 把规则书正文、私密 GM 信息、本地路径或 payload body 写入 package/report。

## Target Layout

Stage 7 目标路径统一在 AstraRPG 顶层下：

```text
Engine/Source/Modules/AstraRPG/
  astra-rpg/
  astra-rpg-core/
  astra-rpg-policy/
  astra-rpg-trpg/
  astra-rpg-runtime-provider/
  astra-rpg-editor/
```

`astra-rpg` 只做 facade/re-export。`astra-rpg-core` 持有 session/intent/effect/ruleset/save DTO。`astra-rpg-policy` 绑定 `astra-policy` 的 Luau host API。`astra-rpg-trpg` 持有 `rpg.trpg` ruleset、dice、check、ruling、seat 和 transcript DTO。`astra-rpg-runtime-provider` 组合 provider lifecycle。`astra-rpg-editor` 只输出 `RuntimeEditorMetadata`。

## Samples

- [AstraRPG AI Town](../samples/astra-rpg-ai-town/README.md)：20 NPC one-day deterministic headless scenario、save/load/replay、memory ledger 和 provider-free replay。
- [CP2020 Local Adapter](../samples/astra-rpg-cp2020-local-adapter/README.md)：CP2020 local-private content adapter、sheet/check/battle resolver skeleton、seat authority 和 transcript redaction，不提交规则书正文或表格。

## Status

AstraRPG 当前是 Stage 7 `SPEC_READY` 目标。仓库还没有 `Engine/Source/Modules/AstraRPG/` 代码、provider crate、sample project 或 release gate 实现。实现完成前，文档只能写 planned target，不能把 Stage 7 或 CP2020 adapter 标成已实现能力。

# AstraRPG Runtime Blueprint

本页把 [AstraRPG Contract](../contracts/rpg-trpg.md) 落到实现边界。Stage 7 只规划 AstraRPG 本地 runtime、TRPG profile、AI Town 和 CP2020 local-private adapter；Server/Client 协议放到 Stage 8。

## Crate Plan

```text
Engine/Source/Runtime/astra-policy/
  shared Luau sandbox, snapshot, trace, manifest, lock, diagnostic

Engine/Source/Modules/AstraRPG/astra-rpg-core/
  RpgSession, RpgIntent, RpgEffect, RpgSheet, CommittedAgentOutput

Engine/Source/Modules/AstraRPG/astra-rpg-policy/
  astra.rpg.* Luau host API, policy bundle manifest, capability validation

Engine/Source/Modules/AstraRPG/astra-rpg-trpg/
  rpg.trpg profile, dice ledger, check/ruling/transcript/seat/privacy DTO

Engine/Source/Modules/AstraRPG/astra-rpg-runtime-provider/
  AstraRpgRuntimeProvider prepare/probe/open/step/save/restore/package/release/editor metadata

Engine/Source/Modules/AstraRPG/astra-rpg-editor/
  Map/Quest/Party/Inventory/Encounter/Behavior Graph/TRPG sheet metadata

Engine/Source/Modules/AstraRPG/astra-rpg/
  facade-only re-export
```

`astra-policy` 必须先从 `astra-vn-policy` 抽出可复用 Luau sandbox/snapshot/trace 机制。AstraVN 保持现有 public type alias 和 migration 说明，避免现有 VN package/save section 被改名。

## Runtime Flow

```text
project target
  -> runtime_provider: astra_rpg
  -> prepare ruleset, policy bundle, source cache and package sections
  -> open RpgSession
  -> RuntimeWorld StateMachine invokes astra.product_runtime.step
  -> AstraRpgRuntimeProvider validates RpgIntent
  -> policy/ruleset returns RpgEffect list
  -> host adapter applies ActionEffect, AwaitToken, event and commands
  -> save/package/release gate consume rpg.* sections
```

Provider 内部可以维护 product-specific cursor，但权威结果只能在固定 tick 边界写入 RuntimeWorld 或 provider save section。Provider 不能持有 Actor 指针、Component mutable ref、Luau VM handle、AI provider secret、Editor widget、native renderer/audio handle 或本地 root。

## Intent And Effects

`RpgIntent` 是所有 player/script/runtime-ai/GM/system action 的入口。IntentValidator 必须输出 allowed/rejected、diagnostic、committed output ref 和 effect list。

`RpgEffect` 只表达 RPG 语义：

- actor sheet/resource/condition patch；
- inventory、quest、encounter、battle 和 faction mutation；
- memory/relationship ledger append；
- runtime event 或 await token request；
- presentation/audio command request；
- committed AI output section ref。

`RpgEffect` 不能直接修改 RuntimeWorld。`astra-rpg-runtime-provider` 把 effect 转换为 `ActionEffect` 或 provider save section，再交给 `DeterministicActionContext` 提交。组件 patch 需要先补齐 runtime 的 component lookup/replace/patch effect，并在 failure 时回滚当前 transition。

## Policy Host

`astra-rpg-policy` 使用 `astra-policy` 执行 Luau。Host API 分两层：

- `astra.rpg.*`：sheet、memory、relationship、location、intent、effect、dice、trace。
- `astra.rpg.trpg.*`：sheet profile、dice visibility、check resolver、ruling、seat、transcript。

每个 host call 返回 value 或 structured diagnostic。Policy source、types、lock、vendor cache 和 capability manifest 必须进入 package evidence。Release profile 缺 lock/source cache 时 blocking。

## Package And Save

Stage 7 package/save section 均使用 `rpg.*` namespace。TRPG profile 使用 `rpg.trpg.*` 子 namespace。所有 section payload 使用 serde/postcard 稳定 DTO；除非有显式 codec，不在 postcard section 字段上使用 `skip_serializing_if`。

Release report 只能记录 schema、section id、hash、byte size、coverage、diagnostic、seat id、ruleset id 和 redaction status。不得记录规则书正文、AI prompt/output body、商业 payload、本地绝对路径或 native handle。

## AI Town

AI Town 是 Stage 7 的公开压力样例。v1 完成目标是 20 NPC，而不是 smoke：

- 每个 NPC 有 sheet、goal stack、memory、relationship/faction 和 agent profile。
- 每个 day tick 的 AI proposal 必须 committed 或 rejected。
- Save/load 后 state/event/presentation/provider hash 一致。
- Replay 不能调用 live AI provider。
- Release gate 通过 `rpg.ai_town.agent_count`、`rpg.intent_validator`、`rpg.agent_provider_free_replay` 和 `rpg.save_load_replay`。

## CP2020 Local-Private Adapter

CP2020 adapter 使用 `content_mode: local_private`。仓库只提供 schema、adapter manifest、resolver interface、diagnostic、public minimal fixture 和 local content import report shape。完整规则书、表格、职业/装备/义体清单和可复原规则 payload 只能由用户本地合法内容包提供。

Local import report 只能写 alias、项目内相对 path、hash、byte size、schema、coverage 和 diagnostic。缺 local manifest、hash mismatch、payload-like 字段、规则正文、表格内容或本地 root 都必须 blocking。

## Stage 8 Protocol

Server/Client 协议属于 Stage 8。Stage 7 只需要本地 seat authority 和 transcript/replay。Stage 8 才新增 `rpg.net.*` DTO、server/client crate、handshake、seat sync、action transcript sync、redacted network audit 和 replay consistency release checks。

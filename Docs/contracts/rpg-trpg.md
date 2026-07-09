# AstraRPG Contract

AstraRPG 是后续 `Game` target 的通用 RPG gameplay runtime provider。它覆盖传统 RPG、AI 自主模拟 RPG，以及桌面规则书式 TRPG 玩法。外部资料里写作 `AstraTRPG` 的能力，在本仓统一落到 AstraRPG 里的 `rpg.trpg` profile/ruleset layer；它不是独立顶层模块、不是独立 runtime provider，也不使用独立 `trpg.*` package namespace。

## Runtime Provider

`AstraRpgRuntimeProvider` 通过 [Game Runtime Provider](game-runtime-provider.md) 接入 `ProductRuntimeProvider`。EngineCore 仍只提供 `RuntimeWorld`、Actor/Component、StateMachine、ActionEffect、AwaitToken、Save/Replay、Asset VFS、Media、Plugin ABI 和 Release Gate。AstraRPG 负责把 RPG 语义转换成可序列化 effect、event、await token、package section、save section 和 release check。

Provider 支持三类 product mode：

- `traditional_rpg`：map、party、inventory、quest、encounter、battle 和 scripted behavior。
- `ai_sim`：agent goal、memory、relationship、intent validation、committed output 和 provider-free replay。
- `trpg`：character sheet、dice、check、ruling、transcript、seat authority 和 privacy/redaction；仍通过 AstraRPG session 执行。

## Public DTO

Stage 7 的 Rust 类型是真正 schema 源。设计要求以下 DTO 只携带 stable id、hash、source ref、schema id、section ref 和 serde/postcard payload，不携带 Luau VM handle、AI provider secret、平台 native handle、本地路径、规则书正文或商业 payload。

| DTO | 用途 |
| --- | --- |
| `RpgSession` | RPG runtime session cursor、ruleset/profile、dirty section 和 replay source |
| `RpgActorProfile` | actor 的 archetype、tag、faction、policy 和 agent metadata |
| `RpgSheet` | attributes、skills、resources、derived stats 和 schema version |
| `RpgIntent` | player/script/runtime-ai/GM/system 提出的动作意图 |
| `RpgEffect` | RPG 语义 effect，最终必须转成 `ActionEffect` 或 provider save section |
| `CommittedAgentOutput` | live AI proposal 的固化记录；replay 只能读取该记录 |
| `RpgEncounter` | encounter/battle/social scene 的确定性状态 |
| `RpgPolicyBundleManifest` | Luau rule policy bundle、capability、lock 和 source cache |
| `RpgTrpgRulesetDescriptor` | `rpg.trpg` ruleset/profile 的入口描述 |
| `RpgTrpgRollRequest` / `RpgTrpgRollResult` | deterministic dice request/result 和 replay token |
| `RpgTrpgCheckRequest` / `RpgTrpgCheckResult` | skill/check resolver 输入输出 |
| `RpgTrpgRuling` | human/AI GM ruling proposal 或 commit 记录 |
| `RpgTrpgTranscriptEntry` | player-visible、GM-only 和 redacted transcript entry |

## Luau Policy

AstraRPG 复用未来 `astra-policy` 的通用 Luau sandbox、snapshot、trace、manifest、lock/vendor cache 和 diagnostic 机制。`astra-vn-policy` 里已经存在的 sandbox/snapshot/trace 能力先迁到通用 crate，再由 VN、RPG 和 EMU policy host 绑定各自 namespace。

Policy 规则：

- 默认无 filesystem、network、process、clipboard、native call。
- 所有权威写入必须通过 host API 产生 `RpgEffect`、`ActionEffect`、runtime event、await token 或 save section。
- 骰子、随机数、时间和 tick 必须来自 deterministic runtime source，不能读取 wall clock 或 Luau `math.random`。
- `RuntimeAi` 输出先进入 `RpgIntent`，通过 IntentValidator 后写入 `CommittedAgentOutput`；save/replay 不重新请求 provider。
- Snapshot 只能保存 nil、bool、integer、string 和 bounded object/table；function、thread、userdata、native handle、非有限数值和本地路径必须 blocking。
- Policy lock/vendor cache 是 release 必需输入；release profile 不能在线解析依赖。

Host API 使用 product namespace，例如：

```luau
astra.rpg.query.sheet(actor_id: string): table
astra.rpg.intent.validate(intent: table): table
astra.rpg.intent.commit(intent: table, output: table): table
astra.rpg.effect.queue(effect: table): table
astra.rpg.effect.patch_component(actor_id: string, schema: string, patch: table): table
astra.rpg.dice.roll(expr: string, meta: table): table
astra.rpg.trace.event(kind: string, fields: table)

astra.rpg.trpg.sheet.get(actor_id: string): table
astra.rpg.trpg.check.resolve(req: table): table
astra.rpg.trpg.ruling.propose(ruling: table): table
astra.rpg.trpg.ruling.commit(ruling: table): table
astra.rpg.trpg.transcript.append(entry: table): table
astra.rpg.trpg.seat.current(): table
```

`rpg.trpg.ruling.commit` 必须检查 seat authority。AI GM 默认只能 propose；只有 project policy 显式允许、且 release gate 有对应 evidence 时，AI referee 才能 commit。

## Package And Save Sections

所有 section 走 `rpg.*` namespace。TRPG profile 使用 `rpg.trpg.*`，不得新增顶层 `trpg.*` section。

| Section | 内容 |
| --- | --- |
| `rpg.session_state` | session cursor、ruleset/profile、dirty section cursor 和 replay source |
| `rpg.ruleset_manifest` | RPG ruleset descriptor、schema version、profile 和 migration policy |
| `rpg.rule_policy_bundle_manifest` | Luau rule policy bundle manifest |
| `rpg.rule_policy_lock` | policy dependency lock/vendor cache hash |
| `rpg.rule_policy_source_cache` | release 可复现的 source cache hash/byte size |
| `rpg.agent_memory_ledger` | agent memory hash ledger、namespace、policy 和 compaction evidence |
| `rpg.committed_agent_output` | provider-free replay 所需的 committed output hash/section ref |
| `rpg.encounter_state` | encounter/battle/social scene state |
| `rpg.quest_state` | quest graph、objective、condition 和 completion evidence |
| `rpg.faction_state` | faction、relationship 和 reputation ledger |
| `rpg.trpg.ruleset_manifest` | TRPG profile/ruleset descriptor |
| `rpg.trpg.character_sheet_schema` | character sheet schema、migrator 和 validation policy |
| `rpg.trpg.dice_ledger` | roll request/result、seed stream、visibility 和 replay token |
| `rpg.trpg.check_ledger` | check/opposed check request/result |
| `rpg.trpg.ruling_ledger` | ruling proposal/commit、referee、reason hash 和 seat evidence |
| `rpg.trpg.transcript` | transcript entry、visibility、redaction 和 hash |
| `rpg.trpg.seat_authority` | local seat、GM/player/AI permissions 和 commit authority |
| `rpg.trpg.privacy_policy` | player-visible、GM-only、private roll 和 redaction policy |

Package sections 只能写项目内相对 path、hash、byte size、schema、coverage 和 diagnostic。CP2020 等 local-private adapter 不能把规则书正文、表格、装备/职业完整清单、payload bytes 或本地 root 写入 package/report。

## Release Checks

Stage 7 新增 release checks，全部保持 planned，直到对应代码和 scenario evidence 落地。

| Check ID | Blocking 条件 |
| --- | --- |
| `runtime_provider.astra_rpg` | 缺 provider binding、descriptor hash/fingerprint 不匹配、profile 不支持或 package section 未声明 |
| `rpg.policy_bundle` | 缺 policy manifest、lock/source cache、capability、schema、source hash 或 vendor cache |
| `rpg.intent_validator` | intent 未经 validator、效果不可序列化、越权 mutation 或诊断缺失 |
| `rpg.committed_agent_output` | live AI output 未固化、prompt/output hash 缺失或 replay 需要 provider |
| `rpg.agent_provider_free_replay` | save/load/replay 过程中重新请求 AI provider |
| `rpg.save_load_replay` | state/event/presentation/provider section hash 不一致 |
| `rpg.ai_town.agent_count` | AI Town v1 少于 20 NPC，或 actor 缺 sheet/goal/memory/profile |
| `rpg.trpg.ruleset_manifest` | ruleset/profile/schema/migrator/capability 缺失 |
| `rpg.trpg.dice_determinism` | dice ledger 缺 deterministic seed stream、roll replay token 或 replay hash 不一致 |
| `rpg.trpg.seat_authority` | GM/player/AI seat 越权 commit、private data 泄露或 authority evidence 缺失 |
| `rpg.trpg.transcript_redaction` | transcript 写入 player-private、GM-only、规则书正文、本地路径或 payload body |
| `rpg.cp2020.local_private_adapter` | CP2020 adapter 直接提交规则书正文/表格/payload，或 local content manifest/hash 不可验证 |

## Server/Client Protocol Boundary

AstraRPG 会自带可扩展 Server/Client 协议，但不进入 Stage 7 完成条件。Stage 8 定义 `rpg.net.*` DTO、session handshake、seat sync、action transcript sync、redacted network audit 和 replay consistency。Stage 7 只要求本地 seat authority、transcript、save/load/replay 和 provider-free replay。

## CP2020 Local-Private Adapter

CP2020 是 local-private adapter 目标，不是仓库内置完整规则书。仓库可以提交：

- adapter manifest schema、content mode、local package manifest 和 hash policy；
- character sheet schema adapter、resolver interface、diagnostic code 和 redaction gate；
- 使用公开最小 fixture 的 social/check/battle smoke scenario；
- local content import 的相对 path/hash/byte size/report shape。

仓库不能提交完整 CP2020 规则正文、表格、职业/装备/义体清单、可复原规则 payload、扫描图、截图、音频、影片或本地绝对路径。需要完整规则时，只能通过用户本地合法内容包提供；report 只记录 manifest、hash、coverage、diagnostic 和 redaction status。

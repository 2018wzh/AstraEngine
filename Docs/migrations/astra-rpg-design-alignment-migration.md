# AstraRPG Design Alignment Migration

本迁移文档只列出当前实现向 AstraRPG 设计对齐的可执行路线。本次文档工作不做代码迁移，不新增 Rust crate，不修改 workspace member，也不把 AstraRPG 写成已实现能力。

## Scope

迁移已有实现：

- `astra-vn-policy` 中可复用的 Luau sandbox、snapshot、trace、manifest、lock/source cache 和 diagnostic 机制。
- `astra-runtime` 中 Actor/Component 和 `ActionEffect` 对组件 patch/replace 的缺口。
- Stage 1/3 已有 plugin/runtime provider registry、`ProductRuntimeProvider` DTO、真实 FFI instance/session lifecycle 和 NativeVN RuntimeWorld action 调用路径。
- Release Gate、stage-test matrix、coverage matrix 和 docs status 的 planned target 口径。

新增实现，不属于已有代码搬迁：

- `Engine/Source/Modules/AstraRPG/` 下的 AstraRPG crates。
- `rpg.trpg` profile/ruleset layer。
- AI Town sample。
- CP2020 local-private adapter。
- Stage 8 Server/Client protocol。

## Migration Order

1. **Extract shared policy runtime**
   - 新增 `Engine/Source/Runtime/astra-policy/`。
   - 从 `astra-vn-policy` 抽出 `PolicyValue`、snapshot serializer、command/query/trace records、sandbox denylist、manifest lock/source-cache DTO 和 diagnostic helpers。
   - `astra-vn-policy` 保留兼容 type alias 和 VN host API，不改 `vn.*` package/save section 名。
   - 验收：`cargo test -p astra-policy`、`cargo test -p astra-vn-policy --test luau_sandbox`、`cargo test -p astra-vn-policy --test luau_mutation`。

2. **Add runtime component patch effects**
   - 在 `ActorStore` 增加 component mutable lookup、component data replace、actor+schema component lookup。
   - 在 `ActionEffect` 增加 component replace/map patch effect。
   - `DeterministicActionContext` 遇到缺 component、非 map patch 或 schema mismatch 必须 blocking，不能静默忽略。
   - 验收：`cargo test -p astra-runtime --test state_machine_tick` 覆盖 patch success、patch type failure、transition rollback 和 other machine isolation。

3. **Generalize product runtime step bridge**
   - 保持现有 `ProductRuntimeProvider` DTO/FFI shape。
   - 将 NativeVN scenario runner 用到的 provider step adapter 抽成 provider-agnostic action bridge。
   - Provider output 只能包含 `ActionEffect`、AwaitToken、presentation/audio command、diagnostic、trace 和 dirty save section。
   - 验收：NativeVN provider tests 继续通过，并新增 provider-agnostic fixture test。

4. **Add AstraRPG core crates**
   - 在 `Engine/Source/Modules/AstraRPG/` 下新增 core/policy/trpg/runtime-provider/editor/facade crates。
   - 先落 DTO、schema、save/package section plan 和 provider descriptor，不接 live AI provider。
   - 验收：`cargo test -p astra-rpg-core`、`cargo test -p astra-rpg-runtime-provider`。

5. **Add RPG policy host**
   - `astra-rpg-policy` 复用 `astra-policy`。
   - 暴露 `astra.rpg.*` host API，所有 mutation 只排队为 `RpgEffect`。
   - 验收：sandbox denied capability、policy snapshot、intent validation、effect serialization 和 provider-free replay tests。

6. **Add `rpg.trpg` profile**
   - 在 `astra-rpg-trpg` 内实现 ruleset descriptor、character sheet schema、deterministic dice ledger、check/ruling ledger、seat authority、privacy policy 和 transcript redaction。
   - 不创建 `AstraTRPG/` 顶层目录，不创建 `AstraTrpgRuntimeProvider`，不使用 `trpg.*` section。
   - 验收：dice determinism、seat authority、transcript redaction 和 save/load/replay tests。

7. **Add public samples and gates**
   - 新增 AI Town 20 NPC headless sample。
   - 新增 CP2020 local-private adapter sample，只提交 schema、manifest、resolver skeleton、public minimal fixture 和 local content import gate。
   - Release Gate 增加 `runtime_provider.astra_rpg`、`rpg.policy_bundle`、`rpg.intent_validator`、`rpg.agent_provider_free_replay`、`rpg.trpg.*` 和 `rpg.cp2020.local_private_adapter` checks。
   - 验收：`astra test run` 的 AI Town one-day scenario、CP2020 local adapter public smoke、`cargo test -p astra-release rpg_gate`。

8. **Defer network protocol to Stage 8**
   - Stage 8 再新增 `rpg.net.*` DTO、server/client crates、handshake、seat sync、transcript sync、redacted network audit 和 replay consistency gate。
   - Stage 7 不以网络传输实现作为完成条件。

## Documentation Rules

- 每完成一个迁移任务，必须同步更新 `Docs/status/implementation-plan.md`、对应 Stage 页面、[stage-test-matrix](../status/stages/stage-test-matrix.md) 和 [coverage-matrix](../status/coverage-matrix.md)。
- 未跑过关联测试和 release report 时，不得把任何 Stage 7/8 work item 标为 `DONE`。
- CP2020 相关文档只能写 local-private adapter、schema、manifest、hash、coverage 和 diagnostic；不能写规则书正文、表格、完整职业/装备/义体清单、扫描图或可复原 payload。

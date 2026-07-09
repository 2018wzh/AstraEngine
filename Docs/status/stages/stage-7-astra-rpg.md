# Stage 7 AstraRPG

Stage 7 adds `AstraRpgRuntimeProvider`, generic RPG runtime DTO, Luau rule policy, AI Town, and the `rpg.trpg` profile. TRPG is an AstraRPG module/profile, not a peer product module. Server/Client protocol is intentionally deferred to Stage 8.

## Exit Criteria

- `AstraRpgRuntimeProvider` is selectable through explicit `runtime_provider: astra_rpg` binding and declares package/save/release/editor metadata.
- Shared `astra-policy` exists, and AstraVN keeps compatibility while RPG policy reuses the same sandbox/snapshot/trace base.
- `RpgIntent` -> IntentValidator -> `RpgEffect` -> `ActionEffect` path is deterministic and observable.
- AI Town 20 NPC one-day headless scenario passes save/load/replay and provider-free replay.
- `rpg.trpg` profile passes deterministic dice, check/ruling ledger, seat authority and transcript redaction gates.
- CP2020 local adapter blocks rulebook payload, table payload, local path leak and unverified local content.

## Work Items

### S7-POLICY-01 Shared Policy Runtime

**Goal:** Extract reusable Luau sandbox/snapshot/trace/manifest/lock functionality into `astra-policy`.

**Depends On:** `S3-LUAU-01`, `S3-LUAU-02`.

**Target Paths:** `Engine/Source/Runtime/astra-policy/` planned, `Engine/Source/Modules/AstraVN/astra-vn-policy/` migration.

**Steps:** Move generic policy value, trace, sandbox denylist and lock/source cache DTO into `astra-policy`; keep VN host API and package section names compatible.

**Done Evidence:** `cargo test -p astra-policy`, `cargo test -p astra-vn-policy --test luau_sandbox`, `cargo test -p astra-vn-policy --test luau_mutation`.

**Linked Test IDs:** `T-S7-POLICY-01`.

### S7-RPG-PROVIDER-01 AstraRPG Runtime Provider

**Goal:** Add `AstraRpgRuntimeProvider` as a planned peer gameplay runtime provider.

**Depends On:** `S3-RUNTIME-PROVIDER-01`, `S7-POLICY-01`.

**Target Paths:** `Engine/Source/Modules/AstraRPG/astra-rpg-runtime-provider/` planned.

**Steps:** Implement descriptor、prepare/probe/open/step/save/restore/package/release/editor metadata and FFI DTO adapter without crossing RuntimeWorld pointers.

**Done Evidence:** `cargo test -p astra-rpg-runtime-provider`, `cargo test -p astra-plugin runtime_provider_registry`, `cargo test -p astra-release rpg_gate`.

**Linked Test IDs:** `T-S7-RPG-PROVIDER-01`.

### S7-RPG-CORE-01 RPG Core DTO

**Goal:** Add `RpgSession`、`RpgIntent`、`RpgEffect`、`RpgSheet` and `CommittedAgentOutput`.

**Depends On:** `S7-RPG-PROVIDER-01`.

**Target Paths:** `Engine/Source/Modules/AstraRPG/astra-rpg-core/` planned.

**Steps:** Add serde/schemars DTO、save/package section plan、intent validator and effect adapter boundary.

**Done Evidence:** `cargo test -p astra-rpg-core`.

**Linked Test IDs:** `T-S7-RPG-CORE-01`.

### S7-RPG-POLICY-01 RPG Luau Policy

**Goal:** Bind `astra-policy` to `astra.rpg.*` host API.

**Depends On:** `S7-POLICY-01`, `S7-RPG-CORE-01`.

**Target Paths:** `Engine/Source/Modules/AstraRPG/astra-rpg-policy/` planned.

**Steps:** Add rule policy manifest、capability validation、intent validation host calls、effect queue and deterministic dice host call.

**Done Evidence:** `cargo test -p astra-rpg-policy`.

**Linked Test IDs:** `T-S7-RPG-POLICY-01`.

### S7-RPG-AI-TOWN-01 AI Town Sample

**Goal:** Prove AI autonomous RPG with 20 NPC one-day deterministic headless scenario.

**Depends On:** `S7-RPG-POLICY-01`.

**Target Paths:** `Examples/AstraRPG/AITown/` planned.

**Steps:** Add actor sheets、goal stack、memory、relationship/faction metadata、daily loop state machine、policy bundle and `one_day_headless.yaml`.

**Done Evidence:** `astra test run Examples/AstraRPG/AITown/Content/Scenarios/one_day_headless.yaml --target ai-town-headless --headless --report target/reports/ai-town.yaml`, plus release gate report.

**Linked Test IDs:** `T-S7-RPG-AI-TOWN-01`.

### S7-RPG-TRPG-01 `rpg.trpg` Profile

**Goal:** Add TRPG ruleset/profile inside AstraRPG.

**Depends On:** `S7-RPG-CORE-01`.

**Target Paths:** `Engine/Source/Modules/AstraRPG/astra-rpg-trpg/` planned.

**Steps:** Add ruleset descriptor、character sheet schema、deterministic dice ledger、check/ruling ledger、seat authority、privacy policy and transcript redaction.

**Done Evidence:** `cargo test -p astra-rpg-trpg`.

**Linked Test IDs:** `T-S7-RPG-TRPG-01`.

### S7-RPG-CP2020-01 CP2020 Local Adapter

**Goal:** Provide CP2020 local-private adapter without committing copyrighted rule content.

**Depends On:** `S7-RPG-TRPG-01`.

**Target Paths:** `Examples/AstraRPG/CP2020LocalAdapter/` planned.

**Steps:** Add adapter manifest、public fixture sheet、resolver skeleton、local content manifest/hash policy、social check and combat smoke scenarios.

**Done Evidence:** `astra test run Examples/AstraRPG/CP2020LocalAdapter/Content/Scenarios/social_check.yaml --target cp2020-local-headless --headless` and `cargo test -p astra-release rpg_cp2020_gate`.

**Linked Test IDs:** `T-S7-RPG-CP2020-01`.

### S7-RPG-GATE-01 RPG Release Gate

**Goal:** Add release checks for AstraRPG, `rpg.trpg`, AI Town and local-private CP2020 adapter.

**Depends On:** `S7-RPG-PROVIDER-01`, `S7-RPG-AI-TOWN-01`, `S7-RPG-TRPG-01`, `S7-RPG-CP2020-01`.

**Target Paths:** `Engine/Source/Developer/astra-release/tests/rpg_gate.rs` planned.

**Steps:** Validate provider binding、policy bundle、intent validator、committed agent output、provider-free replay、dice determinism、seat authority、transcript redaction and payload leak blocking.

**Done Evidence:** `cargo test -p astra-release rpg_gate`.

**Linked Test IDs:** `T-S7-RPG-GATE-01`.

## Status

Stage 7 is `SPEC_READY`. No Stage 7 code, sample project or release gate exists yet.

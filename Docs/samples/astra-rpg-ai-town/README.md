# AstraRPG AI Town Sample

AI Town 是 Stage 7 的公开 AstraRPG 样例规格。它用于验证 AI 自主 RPG 的 deterministic runtime、intent validation、committed output、memory ledger、save/load/replay 和 provider-free replay。

## Target Shape

```text
Examples/AstraRPG/AITown/
  project.yaml
  Content/
    Rules/ai_town.rule_policy.yaml
    Rules/town_life.rules.luau
    Actors/
    Locations/
    Factions/
    StateMachines/agent_daily_loop.yaml
    Scenarios/one_day_headless.yaml
```

## Completion Bar

v1 目标是 20 NPC one-day headless gate。它不是 smoke：

- 每个 NPC 必须有 `rpg.sheet`、goal stack、memory ledger、relationship/faction metadata 和 agent profile。
- 每个 AI proposal 必须进入 `RpgIntent`，由 policy/validator committed 或 rejected。
- 所有 committed output 必须写入 `rpg.committed_agent_output` 或等价 save section ref。
- Save/load 后 state/event/presentation/provider section hash 一致。
- Replay 不得启动 live AI provider。
- Report 不记录 prompt body、AI output body、玩家私密内容、本地路径或 payload。

## Planned Scenario

```yaml
schema: astra.scenario.v1
id: scenario:/astra_rpg/ai_town/one_day_headless
target: ai-town-headless
profile: headless-deterministic
seed: 42
steps:
  - tick: 1
    event: astra.rpg.time.morning
  - tick: 120
    assert:
      path: $.rpg.agent_memory_ledger.count
      gte: 20
  - tick: 240
    save: save:/ai_town/noon
  - tick: 241
    load: save:/ai_town/noon
  - tick: 480
    assert_hash_stable: true
  - tick: 481
    assert:
      path: $.rpg.committed_agent_output.provider_requests_during_replay
      equals: 0
```

## Planned Release Checks

- `runtime_provider.astra_rpg`
- `rpg.policy_bundle`
- `rpg.intent_validator`
- `rpg.ai_town.agent_count`
- `rpg.committed_agent_output`
- `rpg.agent_provider_free_replay`
- `rpg.save_load_replay`

## Status

This sample is planned. No `Examples/AstraRPG/AITown/` project exists yet, and no Stage 7 release gate may treat this page as runnable evidence.

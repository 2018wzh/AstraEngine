# AstraRPG CP2020 Local Adapter Sample

本样例定义 CP2020 风格 TRPG 适配的仓库边界。CP2020 完整规则、表格和内容包不进入仓库；它们只能由用户本地合法内容包提供。仓库内只保留 `rpg.trpg` profile、adapter schema、resolver interface、public minimal fixture、local content manifest/hash 和 redaction gate。

## Target Shape

```text
Examples/AstraRPG/CP2020LocalAdapter/
  project.yaml
  Content/
    Rules/cp2020.adapter.yaml
    Rules/character_sheet.schema.yaml
    Rules/resolvers.policy.luau
    Actors/public_fixture_runner.yaml
    Actors/public_fixture_contact.yaml
    Scenes/public_fixture_scene.yaml
    Scenarios/social_check.yaml
    Scenarios/combat_smoke.yaml
```

## Local Content Boundary

`cp2020.adapter.yaml` may declare:

- `content_mode: local_private`
- adapter id/version/family
- local content manifest schema
- expected hash policy
- allowed resolver hooks
- redaction policy
- required diagnostics

It must not include complete rules text, copied tables, complete profession/equipment/cyberware lists, scans, screenshots, payload bytes or local absolute paths.

## Planned Scenario Coverage

- Social/check scenario: public fixture sheet, deterministic dice, check resolver, transcript entry and redaction status.
- Combat smoke: initiative/action order skeleton, dice ledger, seat authority and replay hash.
- Local import gate: manifest present, hashes match, payload-like fields blocked, local root omitted.

## Planned Release Checks

- `rpg.trpg.ruleset_manifest`
- `rpg.trpg.dice_determinism`
- `rpg.trpg.seat_authority`
- `rpg.trpg.transcript_redaction`
- `rpg.cp2020.local_private_adapter`
- `rpg.agent_provider_free_replay`
- `runtime.replay.determinism`

## Status

This sample is planned. It cannot be used as proof that CP2020 support is implemented, and it cannot contain copyrighted rulebook content when implemented.

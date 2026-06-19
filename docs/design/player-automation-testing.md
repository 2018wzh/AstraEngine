# Player Automation Testing

状态：Implemented NativeVN / Tools slice

## 1. 目标

Player Automation Testing provides a reusable way to simulate player-facing QA flows without making Editor a runtime dependency. The first implementation is centered on `AstraGame` QA and reuses the existing packaged `run` evidence path.

Goals:

- drive packaged or source runtime targets with scripted player actions;
- allow explicit `RuntimeEvent` injection for mixed black-box/runtime tests;
- assert over machine-readable evidence with JSON Pointer;
- emit reports that CTest, CI, release gate tooling, and docs can reference;
- keep the same save/replay/hash evidence chain used by NativeVN.

Non-goals for this slice:

- real device capture for keyboard/gamepad/touch/IME;
- perceptual screenshot/audio diff;
- Editor PIE automation;
- AI/MCP automation;
- long soak farm orchestration.

Those are later hardening layers, not requirements for the NativeVN Tools slice.

## 2. CLI Contract

```powershell
AstraGame QA <target> --plan <file-or-dir> [--case <case_id>] [--backend headless|--backend sdl] [--auto-close] --json
```

Rules:

- `<target>` is the same target accepted by `AstraGame` launcher, usually a packaged `.astrapkg`.
- `--plan` may point to one YAML file or a directory of YAML files.
- `--case` filters by `case_id`.
- If neither validation mode is supplied, the runner defaults to headless validation.
- `AstraGame` QA calls the existing `AstraGame` launcher path for player actions, then evaluates explicit runtime-event steps and assertions.

## 3. Plan Schema

```yaml
schema: astra.test.player_plan.v1
suite_id: astra.nativevn.player
cases:
  - case_id: nativevn_title_walk
    persona: first_time_player
    objective: Start from title and choose a branch.
    steps:
      - kind: player_action
        frame: 4
        action: title_continue
      - kind: runtime_event
        frame: 6
        event:
          event_id: event:/astra.test.player/title_walk_probe
          type: event:/astra.vn.dialogue.say_requested
          category: tools.player_test
          source: { kind: test, id: astra.nativevn.player }
          target: { kind: actor, id: actor:/systems/dialogue }
          payload_schema: astra.test.player_probe.v1
          payload: { probe: title_walk }
    assertions:
      - path: /run/artifacts/playable_vn/system_ui_state/title_continue
        equals: true
      - path: /runtime_events/events
        op: min_count
        min_count: 1
```

Step kinds:

- `player_action`: high-level player actions currently supported by the NativeVN playable evidence path, including `title_continue`, `advance`, `system_menu`, `backlog`, `config`, `save`, `load`, `choose`, `replay_checkpoint`, and `auto_close`.
- `runtime_event`: explicit `RuntimeEvent` JSON parsed through `RuntimeEventFromJson` and submitted through `RuntimeWorld::Tick(RuntimeTickInput)`.

Assertions use `nlohmann::json_pointer` over each case report. Supported operations:

- `exists`
- `equals`
- `matches`
- `min_count`

## 4. Report And Diagnostics

`AstraGame` QA emits:

```yaml
schema: astra.test.player_report.v1
target: build/Saved/Packages/NativeVN.astrapkg
plan: Samples/NativeVN/Tests/player/nativevn_player.yaml
total: 3
passed: 3
failed: 0
cases: []
```

Stable diagnostics:

- `ASTRA_PLAYER_TEST_PLAN_INVALID`
- `ASTRA_PLAYER_TEST_RUNTIME_EVENT_INVALID`
- `ASTRA_PLAYER_TEST_ASSERTION_FAILED`
- `ASTRA_PLAYER_TEST_CASE_FAILED`

## 5. Acceptance

- `Samples/NativeVN/Tests/player/nativevn_player.yaml` covers title, branch, menu/config/save/load, replay checkpoint, and one explicit runtime-event injection.
- CTest runs the successful NativeVN plan and two negative plans for assertion failure and invalid runtime event.
- `AstraPhaseTests` covers the Tools API path.
- `astra doc-check` passes with this design page linked from the design and manual indexes.



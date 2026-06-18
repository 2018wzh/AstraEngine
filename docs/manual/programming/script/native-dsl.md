# Native DSL Reference

Status: Phase 8 production `.astra` path implemented.

## Overview

`.astra` is the canonical production DSL for NativeVN. The syntax follows `docs/design/dsl-design-principle.md`: text-first authoring, embedded graph/timeline structure, stable IDs, explicit scene flow, and schema-first extension commands.

## Key Concepts

- Top-level flow uses `story`, `state`, and `scene`.
- `stage` records the intended final stage state.
- `timeline` records deterministic camera/audio/filter/effect tracks.
- Dialogue can use `alice[pose]: text` or expanded `say`.
- Choices use an explicit target per option.
- Extension commands use `@extension.command` and require a registered schema before release packaging.

## Example

```text
story prologue:
  state alice_route:
    scene station: #@id scene_station
      stage: #@id stage_station
        background native:/Backgrounds/Room #@id cmd_stage_bg

      alice[normal]: Good morning. #@id line_station_001

      choice "Walk?": #@id choice_station
        - "Walk together" -> route_walk #@id choice_walk

      -> route_walk #@id trans_walk
```

## API Reference

- `Astra::Script::ScriptRuntimeHost::CompileNative`
- `Astra::Script::ScriptCommand`
- `Astra::Script::ScriptSourceMap`
- `Astra::Script::ScriptDebugSymbol`

## Troubleshooting

- Missing `#@id` is blocking for scenes, dialogue, choices, timelines, and replay/debug-relevant commands.
- Scenes cannot fall through to the next scene.
- Asset references should use stable URIs such as `native:/Backgrounds/Room`.

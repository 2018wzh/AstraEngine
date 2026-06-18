# AstraVN Runtime

Status: Phase 8 AstraVN completion slice implemented.

## Overview

`AstraVN` is the VN-first vertical runtime module on top of Runtime, Scene, Script, Asset, and Media. It supplies VN event schemas, preset actors/components/state machines, `VnSession`, and a richer `VnSessionSnapshot` for save/replay evidence.

## Key Concepts

- AstraVN is not Core.
- VN output crosses the same `RuntimeEvent` and `PresentationCommand` boundary as other runtime systems.
- `VnSessionSnapshot` now includes route state, dialogue history, backlog, stage state, timeline state, choice state, skip/auto state, event logs, presentation commands, headless capture hashes, and runtime hashes.
- Lua is treated as an extension schema layer; NativeVN story flow is authored in `.astra`.

## Architecture

Design reference: [Script and Presentation](../../../design/script-and-presentation.md).

`AstraVN` depends on public Runtime, Scene, Script, Asset, and Media APIs. It does not expose SDL, GPU, audio, Editor, Lua, AI, or legacy VM internals.

## Programming Guide

```cpp
Astra::AstraVN::VnSession session(44);
auto result = session.RunNative(source, {"title", 0}, diagnostics);
auto snapshot = session.CaptureSnapshot(diagnostics);
```

## API Reference

- `Engine/Runtime/AstraVN/Public/Astra/AstraVN/AstraVN.hpp`
- `Astra::AstraVN::VnSession`
- `Astra::AstraVN::VnSessionSnapshot`
- `Astra::AstraVN::FoundationProfile`

## Examples

`astra run Samples/NativeVN --headless-smoke --json` emits `phase8_script_vn` plus a deprecated `phase4_script_vn` alias. The Phase 8 payload includes Native `.astra` execution, command schemas, Lua extension schemas, source maps, v2 script snapshots, VN state, UI evidence, media evidence, save/restore evidence, and replay route hashes.

## Troubleshooting

- If NativeVN validation fails, first inspect script diagnostics for missing IDs or scene fallthrough.
- Extension commands are schema-first in Phase 8; real Live2D/Emote provider execution remains later provider/plugin work.

# AstraVN Foundation

Status: Phase 4 implemented foundation.

## Overview

`AstraVN` provides the Phase 4 VN-first foundation layer on top of Runtime, Scene, Script, Asset, and Media. It supplies preset actors, components, state machines, VN event schemas, and a headless `VnSession` evidence path.

## Key Concepts

- AstraVN is not Core; it is a vertical runtime module for VN-first workflows.
- VN output crosses the same `RuntimeEvent` and `PresentationCommand` boundary as other runtime systems.
- `VnSessionSnapshot` combines runtime save data, script snapshot, route/dialogue state, event logs, presentation commands, and headless capture hashes.
- Camera and Timeline are represented as foundation schema identifiers only; production Timeline runtime is later work.

## Architecture

Design reference: [Script and Presentation](../../../design/script-and-presentation.md).

`AstraVN` depends on public Runtime, Scene, Script, Asset, and Media APIs. It does not expose SDL, GPU, audio, Editor, Lua, or legacy VM internals.

## Programming Guide

Use `VnSession` for the foundation vertical slice:

```cpp
Astra::AstraVN::VnSession session(44);
auto result = session.RunNative(source, {"opening", 0}, diagnostics);
auto snapshot = session.CaptureSnapshot(diagnostics);
```

## API Reference

- `Engine/Runtime/AstraVN/Public/Astra/AstraVN/AstraVN.hpp`
- `Astra::AstraVN::VnSession`
- `Astra::AstraVN::VnSessionSnapshot`
- `Astra::AstraVN::FoundationProfile`

## Examples

`astra run Samples/NativeVN --headless-smoke --json` emits `phase4_script_vn` with Native/Lua parity, VN events, presentation commands, and save/restore evidence.

## Troubleshooting

- Phase 4 does not implement backlog, skip/auto, route management, full package launch, production replay mismatch localization, or real media output.
- If headless hashes differ between Native DSL and Lua, compare their emitted command streams before debugging Media.

# AstraVN Foundation

Status: Phase 4 implemented foundation with playable v1 sample evidence.

## Overview

`AstraVN` provides the Phase 4 VN-first foundation layer on top of Runtime, Scene, Script, Asset, and Media. It supplies preset actors, components, state machines, VN event schemas, a headless `VnSession` evidence path, and playable v1 sample evidence through `NativeVN` and `ArtemisVN`.

## Key Concepts

- AstraVN is not Core; it is a vertical runtime module for VN-first workflows.
- VN output crosses the same `RuntimeEvent` and `PresentationCommand` boundary as other runtime systems.
- `VnSessionSnapshot` combines runtime save data, script snapshot, route/dialogue state, event logs, presentation commands, and headless capture hashes.
- The tools playable evidence layer adds title/system menu/backlog/save/load/config state, media decode evidence, audio playback evidence, and replay route hashes for sample acceptance.
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

`astra run Samples/NativeVN --headless-smoke --json` emits `phase4_script_vn` and `playable_vn` with Native/Lua parity, VN events, presentation commands, save/restore evidence, UI/system state, media decode evidence, and replay route hashes. `--windowed-smoke` adds SDL/headless `window_present` evidence with the primitive count, decoded image/glyph primitive count, and frame hash used for smoke verification. The SDL path uses libpng RGBA pixels for the sample background, character, and UI overlay, plus HarfBuzz/FreeType speaker and dialogue glyph layers. When the target is `.astrapkg`, `window_texture_sources`, `window_glyph_sources`, and audio `decoded_payloads` record package payload reads instead of source-file reads. OGG/Vorbis evidence uses libvorbisfile for decode and miniaudio remains the selected audio backend/mixer evidence. `ArtemisVN` emits the same schema plus a local fixture report for copied Artemis resources; the original `bgm113.ogg` remains a local fixture compatibility gap for the in-memory vorbisfile path while Artemis SE and voice payloads decode.

## Troubleshooting

- Phase 4 playable v1 covers backlog/config/save/load evidence for the two demo samples; skip/auto hooks, production replay mismatch localization, full `.ast` compatibility, and Editor workflows remain later work.
- If headless hashes differ between Native DSL and Lua, compare their emitted command streams before debugging Media.

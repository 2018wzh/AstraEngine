# Save/Replay Guide

Status: Phase 5 Runtime Core evidence slice.

## Overview

Save/replay captures headless runtime state for stable world snapshots, scheduler state, deterministic event hashes, and replay comparison evidence.

## Key Concepts

- Compatibility save schema: `astra.runtime.snapshot.v1`.
- Phase 5 save container schema: `astra.runtime.save_container.v2`.
- Replay foundation schema: `astra.runtime.replay.v1`.
- Snapshot sections include frame counters, event sequence, seed, world snapshot, Director state, scheduler tasks, replay events, and hashes.
- Save v2 uses JSON section manifests with hash evidence and optional zstd-compressed section payloads.
- Hashes use a stable deterministic FNV-1a foundation hash, not implementation-defined `std::hash`.

## Architecture

Design reference: [Runtime Core](../../../design/runtime-core.md).

## Programming Guide

Call `RuntimeWorld::Save()` for a compatibility versioned snapshot and `RuntimeWorld::Load()` to restore it. Call `RuntimeWorld::SaveV2(true)` for a compressed sectioned save container. Call `RuntimeWorld::CaptureReplay()` for a replay report with recorded runtime events, checkpoints, and hashes.

## API Reference

- `Astra::Runtime::RuntimeSnapshot`
- `Astra::Runtime::RuntimeHashes`
- `Astra::Runtime::SaveContainerV2`
- `Astra::Runtime::SchedulerSnapshot`
- `Astra::Runtime::RuntimeWorld::Save`
- `Astra::Runtime::RuntimeWorld::SaveV2`
- `Astra::Runtime::RuntimeWorld::Load`
- `Astra::Runtime::RuntimeWorld::CaptureReplay`

## Examples

Compiled save/load, compressed save v2, scheduler restore, replay mismatch, RuntimeStress, and NativeVN hash coverage lives in `Engine/Tests/PhaseTests.cpp`.

## Troubleshooting

- Do not add C++ pointers, EnTT entities, native handles, Editor widgets, or provider-private state to snapshot payloads.
- Section placeholders exist for script, media, resources, AI committed output, and module extension state. Later provider phases must fill them with production payloads.
- Full UI tooling for replay mismatch inspection remains later production work.

# Save/Replay Guide

Status: Runtime-only production save/replay evidence.

## Overview

Save/replay captures headless runtime state for stable world snapshots, scheduler state, Director arbitration, media logical state, extension state placeholders, deterministic event hashes, and replay comparison evidence.

## Key Concepts

- Compatibility save schema: `astra.runtime.snapshot.v1`.
- Production save container schema: `astra.runtime.save_container.v2`.
- Replay foundation schema: `astra.runtime.replay.v1`.
- Replay stream schema: `astra.runtime.replay_stream.v1`.
- Snapshot sections include frame counters, event sequence, seed, world snapshot, Director state, scheduler tasks, replay events, and hashes.
- Save v2 uses section descriptors with owner module, payload schema, required flag, hash evidence, recovery policy, and optional zstd-compressed section payloads.
- Hashes use a stable deterministic FNV-1a foundation hash, not implementation-defined `std::hash`.

## Architecture

Design reference: [Runtime Core](../../../design/runtime-core.md).

## Programming Guide

Call `RuntimeWorld::Save()` for a compatibility versioned snapshot and `RuntimeWorld::Load()` to restore it. Call `RuntimeWorld::SaveV2(true)` for a compressed sectioned save container. Required runtime-only sections include runtime world, scene actors, event bus, scheduler, Director, script runtime, presentation state, media logical state, module extension state, and optional AI committed output. Call `RuntimeWorld::CaptureReplay()` or build a `ReplayStream` for replay records with checkpoints and hashes.

## API Reference

- `Astra::Runtime::RuntimeSnapshot`
- `Astra::Runtime::RuntimeHashes`
- `Astra::Runtime::SaveContainerV2`
- `Astra::Runtime::SaveSectionDescriptor`
- `Astra::Runtime::SaveMigrationEdge`
- `Astra::Runtime::SchedulerSnapshot`
- `Astra::Runtime::ReplayStream`
- `Astra::Runtime::RuntimeWorld::Save`
- `Astra::Runtime::RuntimeWorld::SaveV2`
- `Astra::Runtime::RuntimeWorld::Load`
- `Astra::Runtime::RuntimeWorld::CaptureReplay`

## Examples

Compiled save/load, compressed save v2, section descriptor, scheduler restore, replay stream, replay mismatch, RuntimeStress, and NativeVN hash coverage lives in `Engine/Tests/PhaseTests.cpp`.

## Troubleshooting

- Do not add C++ pointers, EnTT entities, native handles, Editor widgets, or provider-private state to snapshot payloads.
- Missing required section or migration evidence is a release-gate concern, not a best-effort load path.
- Full UI tooling for replay mismatch inspection remains later Editor/debugger work.

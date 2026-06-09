# Foundation Save/Replay Guide

Status: Phase 2 Foundation.

## Overview

Foundation save/replay captures enough headless runtime state to prove stable world snapshots and deterministic event hashes.

## Key Concepts

- Save schema: `astra.runtime.snapshot.v1`.
- Replay foundation schema: `astra.runtime.replay.v1`.
- Snapshot sections include frame counters, event sequence, seed, world snapshot, Director state, replay events, and hashes.
- Snapshots use JSON and Core `VersionedDocument`.
- Hashes use a stable deterministic FNV-1a foundation hash, not implementation-defined `std::hash`.

## Architecture

Design reference: [Runtime Core](../../../design/runtime-core.md).

## Programming Guide

Call `RuntimeWorld::Save()` for a versioned snapshot and `RuntimeWorld::Load()` to restore it. Call `RuntimeWorld::CaptureReplay()` for a foundation replay report with recorded runtime events and hashes.

## API Reference

- `Astra::Runtime::RuntimeSnapshot`
- `Astra::Runtime::RuntimeHashes`
- `Astra::Runtime::RuntimeWorld::Save`
- `Astra::Runtime::RuntimeWorld::Load`
- `Astra::Runtime::RuntimeWorld::CaptureReplay`

## Examples

Compiled save/load and hash smoke coverage lives in `Engine/Tests/Phase1Tests.cpp`.

## Troubleshooting

- Do not add C++ pointers, EnTT entities, native handles, Editor widgets, or provider-private state to snapshot payloads.
- Full save migration, compression, AI committed output, script/timeline/resource state, and replay mismatch localization remain later production work.

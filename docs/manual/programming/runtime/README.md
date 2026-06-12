# Runtime Programming Guide

Status: Phase 5 Runtime Core evidence slice.

## Overview

The Runtime module provides a headless facade that combines `ActorWorld`, deterministic event ordering, serializable scheduler tasks, StateMachine transitions, Director state, and save/load/replay hashes.

## Key Concepts

- `RuntimeWorld` is a facade, not a global singleton.
- Runtime events are DTOs with stable type IDs, priority, sequence numbers, frame indices, endpoints, payload schema, payload, and trace data.
- StateMachine transitions are target-aware for actor events and support broadcast events for presentation/VN coordination.
- Scheduler tasks are serializable DTOs with event/time/asset/script/debugger wait conditions, cancellation policy, and save/restore state.
- ControlPolicy supports allow, queue, reject, interrupt policy, and priority inheritance checks for locked channels.
- Director state records phase, timeline/choice locks, AI permission window, player input window, and arbitration log DTOs.

## Architecture

Design references:

- [Runtime Core](../../../design/runtime-core.md)
- [Actor / Component / ECS Hybrid](../../../design/actor-component-ecs-hybrid.md)

## Programming Guide

Register a state machine definition, emit a queued or deferred event, then tick:

```cpp
Astra::Runtime::RuntimeWorld runtime(1234);
runtime.RegisterStateMachine(definition);
runtime.Emit(event, Astra::Runtime::RuntimeEventMode::Queued, diagnostics);
runtime.Tick(diagnostics);
```

Use `Save()` to produce the compatibility `VersionedDocument` with schema `astra.runtime.snapshot.v1`. Use `SaveV2()` for `astra.runtime.save_container.v2` section manifests with optional zstd-compressed JSON sections, then `Load(const SaveContainerV2&)` to restore runtime state.

Use `ScheduleTask()` and `CancelTask()` for serializable scheduler work. Use `EvaluateControlPolicy()` for owner/channel checks. Use `CaptureReplay()` for replay DTOs with state, event, presentation hashes, and checkpoints.

## API Reference

- `Engine/Runtime/Runtime/Public/Astra/Runtime/Runtime.hpp`
- `Astra::Runtime::ControlPolicyRequest`
- `Astra::Runtime::RuntimeTask`
- `Astra::Runtime::SaveContainerV2`
- `Astra::Runtime::RuntimeReplay`

## Examples

Compiled Phase 5 examples live in `Engine/Tests/PhaseTests.cpp`, including RuntimeStress coverage for 1000 actors, scheduled tasks, save/load, and deterministic hashes.

## Troubleshooting

- Script, media, resource override, AI committed output, and module extension sections exist as JSON save sections; production providers still need to populate richer payloads in later phases.
- Replay mismatch reports localize hash category; deeper script-command and presentation-command diff viewers remain later tooling work.

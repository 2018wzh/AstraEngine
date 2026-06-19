# Runtime Programming Guide

Status: Runtime-only UE-class core evidence slice.

## Overview

The Runtime module provides a headless facade that combines `ActorWorld`, deterministic event ordering, serializable scheduler tasks, StateMachine transitions, Director arbitration, sectioned save/load, replay streams, and frame result hashes.

## Key Concepts

- `RuntimeWorld` is a facade, not a global singleton.
- Runtime events are DTOs with stable type IDs, priority, sequence numbers, frame indices, endpoints, payload schema, payload, and trace data.
- StateMachine transitions are target-aware for actor events and support broadcast events for presentation/VN coordination.
- Scheduler tasks are serializable DTOs with event/time/asset/script/debugger wait conditions, cancellation policy, and save/restore state.
- ControlPolicy supports allow, queue, reject, interrupt policy, and priority inheritance checks for locked channels.
- Director state records phase, timeline/choice locks, AI permission window, player input window, and arbitration log DTOs.
- `RuntimeTickInput` and `RuntimeFrameResult` are the production tick contract for CLI, packaged runtime, replay, and future Editor PIE/debugger callers.

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

Use `Tick(const RuntimeTickInput&, DiagnosticSink&)` when the caller needs frame evidence. It returns frame index, event sequence range, completed scheduler task IDs, presentation command DTOs, and state/event/presentation hashes.

Use `Save()` to produce the compatibility `VersionedDocument` with schema `astra.runtime.snapshot.v1`. Use `SaveV2()` for `astra.runtime.save_container.v2` section descriptors and optional zstd-compressed JSON sections, then `Load(const SaveContainerV2&)` to restore runtime state.

Use `ScheduleTask()` and `CancelTask()` for serializable scheduler work. Use `EvaluateControlPolicy()` for actor owner/channel checks and `ArbitrateDirector()` for timeline/choice/AI permission-window conflicts. Use `CaptureReplay()` and `ReplayStream` DTOs for replay records, hashes, and checkpoints.

## API Reference

- `Engine/Runtime/Runtime/Public/Astra/Runtime/Runtime.hpp`
- `Astra::Runtime::ControlPolicyRequest`
- `Astra::Runtime::RuntimeTask`
- `Astra::Runtime::SaveContainerV2`
- `Astra::Runtime::RuntimeTickInput`
- `Astra::Runtime::RuntimeFrameResult`
- `Astra::Runtime::DirectorArbitrationRequest`
- `Astra::Runtime::ReplayStream`
- `Astra::Runtime::RuntimeReplay`

## Examples

Compiled examples live in `Engine/Tests/PhaseTests.cpp`, including RuntimeStress coverage for 1000 actors, scheduled tasks, Director arbitration, save/load, replay streams, and deterministic hashes.

## Troubleshooting

- Script, media logical state, AI committed output, and module extension state are explicit save sections. Runtime-only release gate validates that the sections and hashes exist; richer Editor viewers remain later tooling work.
- Replay mismatch reports localize hash category and object/provider/source hints; full visual UI diff tooling remains outside runtime core.



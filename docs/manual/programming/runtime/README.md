# Runtime Programming Guide

Status: Phase 2 Foundation.

## Overview

`AstraRuntime` provides a headless runtime facade that combines `ActorWorld`, `RuntimeEventBus`, basic StateMachine transitions, Director state, and foundation save/load/replay hashes.

## Key Concepts

- `RuntimeWorld` is a facade, not a global singleton.
- Runtime events are DTOs with stable type IDs, sequence numbers, frame indices, endpoints, payload schema, payload, and trace data.
- StateMachine foundation transitions actor-owned `astra.state_machine` component data.
- ControlPolicy foundation supports allow, queue, and reject decisions for locked channels.
- Director state records foundation conflict flags; full arbitration is later production work.

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

Use `Save()` to produce a `VersionedDocument` with schema `astra.runtime.snapshot.v1`. Use `Load()` to restore foundation runtime state.

Use `EvaluateControlPolicy()` for foundation owner/channel checks. Use `CaptureReplay()` for the foundation replay DTO with stable state, event, and presentation hashes.

## API Reference

- `Engine/Runtime/Runtime/Public/Astra/Runtime/Runtime.hpp`
- `Astra::Runtime::ControlPolicyRequest`
- `Astra::Runtime::RuntimeReplay`

## Examples

Compiled Phase 2 examples live in `Engine/Tests/Phase1Tests.cpp`.

## Troubleshooting

- Variable presentation, asset waits, script state, timeline state, AI committed output, and module extension state are not yet saved.
- Replays currently prove deterministic foundation hashes; production mismatch localization is later work.

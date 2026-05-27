# ADR 0004: Bevy-Style ECS Runtime Internals

Status: Accepted

## Context

AstraEngine needs efficient and composable runtime state management for stage objects, dialogue state, transitions, audio requests, input, save snapshots, editor preview, headless tests, compatibility runtimes, and future constrained Runtime AI. The current design keeps Runtime Services as the stable shared layer, but service implementations can become difficult to extend if every subsystem owns bespoke state and update order.

The project already includes EnTT as a dependency. Bevy's ECS architecture provides useful design ideas around World, Entity, Component, Resource, System, Schedule, and Plugin composition, but AstraEngine is a modern C++ engine and must not introduce a Rust or Bevy runtime dependency.

## Decision

Runtime Services will use a Bevy-style ECS model internally, implemented with EnTT in C++.

The ECS model is an internal execution model only:

- `RuntimeCommand` remains the stable protocol inside the Astra Runtime for Astra scripts, Story Graph, optional AI hooks, debugging, logs, and compatibility adapters that translate external script/timeline events at runtime.
- `StageService`, `DialogueService`, `ChoiceService`, `AudioService`, `AssetService`, `InputService`, `SaveService`, and `LocalizationService` remain the public facades.
- EnTT types must not appear in public interfaces consumed by VN DSL, Editor modules, Compatibility plugins, AI plugins, or project gameplay code.
- Runtime Services own the ECS World and schedule execution.

The runtime schedule uses fixed phases:

```text
Input
Script
CommandApply
Animation
Audio
RenderExtract
SaveSnapshot
Cleanup
```

## Consequences

- Stage sprites, backgrounds, transitions, dialogue display, audio requests, and short-lived runtime effects should be modeled as entities and components where that improves locality or composition.
- Project-wide or frame-wide state such as input, save state, dialogue history, audio bus state, runtime config, and asset registry access should be modeled as resources.
- Services should read from and write to ECS data instead of owning separate duplicated state.
- Save/Load must serialize deterministic World and Resource state, not EnTT implementation details.
- Headless tests can execute the schedule without Renderer2D or AudioCore by replacing extract/IO systems.
- If EnTT becomes unsuitable, a later ADR must document the replacement while preserving public Runtime Services interfaces.

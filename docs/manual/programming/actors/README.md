# Actor/Component Programming Guide

Status: Phase 2 Foundation.

## Overview

`AstraScene` provides the headless Actor/Component foundation for runtime, tools, future Editor inspectors, MCP field editing, and save/replay. It is a foundation slice, not the full production prefab and lifecycle system.

## Key Concepts

- `ActorWorld` owns actors and component data.
- `ActorId`, `ActorTypeId`, `ComponentId`, `ComponentDescriptor`, `ActorHandle`, DTOs, and snapshots are public contracts.
- EnTT is used only as private local storage in implementation files.
- Public APIs never expose `entt::entity`, registry internals, C++ Actor pointers, native handles, or Editor widgets.
- Component payloads use JSON so PropertySystem, diagnostics, source schemas, and future tools can share one data shape.

## Architecture

Design references:

- [Actor / Component / ECS Hybrid](../../../design/actor-component-ecs-hybrid.md)
- [Runtime Core](../../../design/runtime-core.md)
- [Implementation Coverage](../../../design/implementation-coverage.md)

## Programming Guide

Create actors with stable IDs and type IDs, add JSON component data, then activate the handle:

```cpp
Astra::Scene::ActorDescriptor descriptor;
descriptor.id = Astra::Core::ParseStableId("actor:/characters/alice").Value();
descriptor.type_id = Astra::Core::ParseStableId("type:/astra.vn.character").Value();
descriptor.name = "Alice";
auto handle = world.Spawn(descriptor, diagnostics);
world.Activate(handle.Value(), diagnostics);
```

Use `Snapshot()` for save/debug/inspector data. Treat handles as generation-checked runtime references, not persistent source identifiers.

Use `FoundationComponentDescriptors()` to discover the built-in foundation component schemas for Transform2D, Tag, Lifetime, Blackboard, ControlPolicy, and StateMachine data. Use `CreateHeadlessLocalEcsPack()` when a test or future system needs to prove sync-in/update/sync-out behavior without exposing ECS entities.

## API Reference

- `Engine/Runtime/Scene/Public/Astra/Scene/Scene.hpp`
- `Astra::Scene::ComponentDescriptor`
- `Astra::Scene::CreateHeadlessLocalEcsPack`

## Examples

Compiled Phase 2 examples live in `Engine/Tests/PhaseTests.cpp`.

## Troubleshooting

- Use `ActorId` in saves, events, scripts, and snapshots.
- Do not store C++ pointers, EnTT entities, registry internals, renderer/audio handles, or Editor-only objects in component data.
- Prefab, variants, full lifecycle hooks, and migration hardening remain production-phase work.

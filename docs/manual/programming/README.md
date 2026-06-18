# Programming

Status: NativeVN Phase 8 full playable evidence. Foundation public headers are implemented for Core, Platform, ModuleRuntime, PropertySystem, Scene, Runtime, Asset, Media, Script, and AstraVN; engine libraries are dynamic-only DLLs. NativeVN and the local TsuiNoSora fixture now have CLI package/run/replay/inspect evidence plus playable VN state, UI/system state, command schema/source-map evidence, generated or copied PNG/OGG/font media evidence, embedded package payloads, PackageReader random-access/chunked-read/mount evidence, mature media backend capability reports, local DDC artifact execution, corruption recovery, and package integrity diagnostics; Editor, AI/MCP, standalone AstraEmu Toolkit, full Artemis VM compatibility, real Live2D/Emote SDK execution, and production release gates remain planned.

## Overview

This section will become the main programming guide for Core, Module, Actor/Component, RuntimeEvent, StateMachine, Asset, Media, Script, and AstraVN development.

## Key Concepts

- Core owns diagnostics, logging, config, path, time, serialization, stable IDs, and PropertySystem.
- Modules cross the stable boundary through C ABI and opaque handles.
- Actor/Component is the public runtime object model; ECS is only a local implementation boundary.
- Script, AI, and AstraEmu communicate through runtime events and provider contracts, not Core dependencies.

## Architecture

Primary design references:

- [Foundation Core / Platform / Property](../../design/foundation-core-platform-property.md)
- [Extension and Module System](../../design/extension-and-module-system.md)
- [Actor / Component / ECS Hybrid](../../design/actor-component-ecs-hybrid.md)
- [Runtime Core](../../design/runtime-core.md)
- [Script and Presentation](../../design/script-and-presentation.md)

## Programming Guide

Implemented foundation guides:

- [Core Programming Guide](core/README.md)
- [Module ABI Reference](modules/abi.md)
- [Plugin Authoring Guide](modules/plugin-authoring.md)
- [Engine Module Slot Guide](modules/engine-module-slots.md)
- [Property System Guide](properties/README.md)
- [Actor/Component Programming Guide](actors/README.md)
- [Runtime Programming Guide](runtime/README.md)
- [Runtime Event Guide](runtime/events.md)
- [StateMachine Guide](runtime/state-machines.md)
- [Foundation Save/Replay Guide](runtime/save-replay.md)
- [Asset Foundation](assets/README.md)
- [VFS And Sidecars](assets/vfs-and-sidecars.md)
- [Script Foundation](script/README.md)
- [Native DSL Foundation](script/native-dsl.md)
- [Lua Host API Foundation](script/lua-host-api.md)
- [AstraVN Foundation](astravn/README.md)

Planned later guides include production Media Provider, Script debugger/hot reload, Graph/Timeline, and production AstraVN workflows.

## API Reference

Foundation headers are indexed in [API Reference](../api/README.md). Later public contracts remain authoritative in design documents until implemented.

## Examples

Current examples include the NativeVN playable route and TsuiNoSora local playable route plus source asset sidecars, AssetRegistry/dependency graph evidence, DDC artifact write/reuse/corruption recovery, embedded package payload read/mount evidence, package manifest integrity, save/load evidence, UI/system evidence, and golden replay evidence. Planned examples include a Core diagnostics packet, a plugin descriptor, a module C ABI entrypoint, and production ScriptParity coverage.

## Troubleshooting

- Keep public ABI examples free of STL ownership, native handles, Editor widgets, and C++ Actor pointers.
- Do not use VN, AI, Lua, or legacy concepts in Core examples.
- Mark examples as planned unless they compile in the current tree.

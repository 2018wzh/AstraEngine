# API Reference

Status: Phase 4 scaffold. Foundation public runtime headers are present for Core, Platform, ModuleRuntime, PropertySystem, Scene, Runtime, Asset, Media, Script, AstraVN, and Tools. Phase 1 Foundation gate APIs are implemented for Core/Platform/ModuleRuntime/PropertySystem; later production runtime systems remain planned.

## Overview

This section will index stable public headers and public contracts. During Phase 0 it records what must be referenced once implementation begins.

## Key Concepts

- Public headers belong under `Public/Astra/<ModuleName>/`.
- Cross-ABI contracts use fixed-width scalars, UTF-8 buffers with lengths, POD descriptors, opaque handles, callbacks, and explicit ownership.
- Public API must not expose SDL, OS handles, renderer/audio native handles, Editor widgets, STL ownership, or C++ Actor/Component pointers across ABI.

## Architecture

Authoritative target contracts are described in:

- [Foundation Core / Platform / Property](../../design/foundation-core-platform-property.md)
- [Extension and Module System](../../design/extension-and-module-system.md)
- [Asset Pipeline](../../design/asset-pipeline.md)
- [Media Runtime](../../design/media-runtime.md)
- [Script and Presentation](../../design/script-and-presentation.md)
- [Architecture](../../design/architecture.md)

## Programming Guide

When a public header is added, update this index with:

- Header path.
- Owning module.
- Stability level.
- Related design section.
- Test or schema evidence.

## API Reference

Implemented foundation entries:

- `Engine/Runtime/Core/Public/Astra/Core/Types.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Diagnostics.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Error.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Profiling.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Logging.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Config.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/StableId.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Serialization.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Path.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/Time.hpp`
- `Engine/Runtime/Core/Public/Astra/Core/BuildInfo.hpp`
- `Engine/Runtime/Platform/Public/Astra/Platform/Platform.hpp`
- `Engine/Runtime/ModuleRuntime/Public/Astra/ModuleRuntime/ModuleAbi.h`
- `Engine/Runtime/ModuleRuntime/Public/Astra/ModuleRuntime/ModuleRuntime.hpp`
- `Engine/Runtime/PropertySystem/Public/Astra/PropertySystem/PropertySystem.hpp`
- `Engine/Runtime/Scene/Public/Astra/Scene/Scene.hpp`
- `Engine/Runtime/Runtime/Public/Astra/Runtime/Runtime.hpp`
- `Engine/Runtime/Asset/Public/Astra/Asset/Asset.hpp`
- `Engine/Runtime/Media/Public/Astra/Media/Media.hpp`
- `Engine/Runtime/Script/Public/Astra/Script/Script.hpp`
- `Engine/Runtime/AstraVN/Public/Astra/AstraVN/AstraVN.hpp`
- `Engine/Programs/astra/Public/Astra/Tools/Tools.hpp`

Phase 3 Media provider/release-gate foundation APIs are in `Media.hpp`:

- `MediaProviderDescriptor`
- `MediaReleaseGateRequest`
- `MediaReleaseGateReport`
- `FoundationMediaProviders()`
- `ValidateMediaProviderDescriptor()`
- `ValidateMediaReleaseGate()`

Phase 1 production Foundation gate APIs are in Core, Platform, ModuleRuntime, PropertySystem, and Tools headers:

- `DiagnosticCodeRegistry`
- `ReleasePolicy`
- `FoundationGateReport`
- `ConfigStack::ResolveForProfile()`
- `ApplyUnknownFieldPolicy()`
- `DynamicLibraryHandle`
- `CrashCaptureContext`
- `ServiceResolveAudit`
- `ValidateModuleReleaseGate()`
- `PropertyWriteRequest`
- `PropertyWriteResult`
- `TypeRegistry::ValidateSchemaVersion()`
- `TypeRegistry::EvaluateWrite()`
- CLI `foundation_core_gate` artifact in `Astra::Tools::Validate()`

Phase 4 Script/AstraVN foundation APIs are in `Script.hpp` and `AstraVN.hpp`:

- `ScriptRuntimeHost`
- `ScriptEventBridge`
- `CompiledScript`
- `ScriptSnapshot`
- `FoundationScriptProviders()`
- `VnSession`
- `VnSessionSnapshot`
- `FoundationProfile()`
- `FoundationStateMachines()`

Planned later entries include production Asset Pipeline, production Media backend providers, full Script debugger/hot reload/Graph/Timeline, production AstraVN package launch, Editor, AI/MCP, and Release Gate APIs.

## Examples

Compiled examples live in `Engine/Tests/Phase1Tests.cpp` and `Engine/Plugins/Examples/Phase1Example/Source/Phase1Example.cpp`.

## Troubleshooting

- If a manual page references a public contract, ensure a design document also references it.
- If a header exposes forbidden ABI types, treat that as a release-blocking issue when ABI scans exist.

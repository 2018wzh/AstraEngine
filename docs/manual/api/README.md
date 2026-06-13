# API Reference

Status: NativeVN runtime evidence plus Phase 6 Asset Pipeline. Foundation public runtime headers are present for Core, Platform, ModuleRuntime, PropertySystem, Scene, Runtime, Media, Script, AstraVN, and Tools; Asset now includes production importer/cooker/DDC/package contracts. Engine libraries are dynamic-only `Astra*` DLLs with generated per-module export headers under `Astra/<Module>/Export.hpp`. Later media/editor/AI runtime systems remain planned.

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

## Implementation Module Index

Current concrete implementation slices:

- `Engine/Runtime/Asset/Private/Asset.cpp`: shared asset helpers, registry and schema handling, URI/hash utilities, and the remaining public asset entry points.
- `Engine/Runtime/Asset/Private/AssetCook.cpp`: builtin importer and cook processor descriptors, cook pipeline dispatch, DDC handling, and asset release-gate checks.
- `Engine/Runtime/Asset/Private/AssetSerialization.cpp`: asset registry, manifest, package, mount, and hot-reload JSON/binary serialization helpers.
- `Engine/Programs/astra/Private/DocCheck.cpp`: documentation gate checks for required manual pages, markdown links, required design documents, and stale wording scans.
- `Engine/Programs/astra/Private/Tools.cpp`: shared CLI command plumbing and evidence helpers for validate, inspect, import, cook, package, run, and replay workflows.
- `Engine/Runtime/Media/Private/Media.cpp`: media backend probing, image/audio/font decode, filter validation, and media release-gate evaluation.
- `Engine/Runtime/Media/Private/MediaSerialization.cpp`: media presentation and report JSON serialization for commands, render graphs, frame captures, and backend capability reports.
- `Engine/Runtime/Runtime/Private/Runtime.cpp`: runtime event bus, scheduler, state machine wiring, control policy, snapshot capture, and world lifecycle.
- `Engine/Runtime/Runtime/Private/RuntimeSerialization.cpp`: runtime JSON serialization/deserialization for events, snapshots, save containers, and replay comparison reports.

As more oversized modules are split, add one line per concrete slice here with a short functional summary. Keep this index honest: do not list placeholder or forwarding-only translation units.

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
- `MediaBackendLibrary`
- `MediaBackendCapabilityReport`
- `ImageDecodeReport`
- `FoundationMediaProviders()`
- `ValidateMediaProviderDescriptor()`
- `ValidateMediaReleaseGate()`
- `ProbeMediaBackendCapabilities()`
- `InspectImageBytes()`

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

Planned later entries include production Media backend providers, full Script debugger/hot reload/Graph/Timeline, production AstraVN authoring workflow, Editor, AI/MCP, and broader runtime release/observability APIs.

Phase 6 Asset and NativeVN runtime evidence APIs now also include:

- `Astra::Asset::ImportRequest`
- `Astra::Asset::ImporterDescriptor`
- `Astra::Asset::IAssetImporter`
- `Astra::Asset::DdcKey`
- `Astra::Asset::CookRequest`
- `Astra::Asset::CookArtifactDescriptor`
- `Astra::Asset::ICookProcessor`
- `Astra::Asset::CookPipelineOptions`
- `Astra::Asset::CookManifest`
- `Astra::Asset::DerivedDataCacheEntry`
- `Astra::Asset::DdcCleanReport`
- `Astra::Asset::PackageWriter`
- `Astra::Asset::PackagePayloadEntry`
- `Astra::Asset::PackagePayloadRef`
- `Astra::Asset::PackagePayloadChunk`
- `Astra::Asset::PackageManifest`
- `Astra::Asset::PackageMountPolicy`
- `Astra::Asset::PackageMount`
- `Astra::Asset::PackageReader`
- `Astra::Asset::AssetReleaseGateReport`
- `Astra::Asset::HotReloadTransaction`
- `Astra::Asset::PackageReader::ReadPayloadBytes()`
- `Astra::Asset::PackageReader::ReadPayloadChunks()`
- `Astra::Asset::PackageReader::ReadPayloadText()`
- `Astra::Asset::PackageReader::MountPackage()`
- `Astra::Asset::CookAssetRegistry()`
- `Astra::Asset::CleanDerivedDataCache()`
- `Astra::Asset::ValidateAssetReleaseGate()`
- `Astra::Asset::PlanHotReloadTransaction()`
- `Astra::Asset::ComputeCookManifestHash()`
- `Astra::Asset::ComputePackageManifestHash()`
- `Astra::Asset::ComputeProviderFeatureHash()`
- `Astra::Runtime::SaveContainer`
- `Astra::Runtime::ReplayComparisonReport`
- `Astra::Tools::Import()`
- `Astra::Tools::Replay()`

## Examples

Compiled examples live in `Engine/Tests/PhaseTests.cpp` and `Engine/Plugins/Examples/Phase1Example/Source/Phase1Example.cpp`.

## Troubleshooting

- If a manual page references a public contract, ensure a design document also references it.
- If a header exposes forbidden ABI types, treat that as a release-blocking issue when ABI scans exist.

# ADR 0009: Compatibility via Existing Extension Mechanisms

Status: Accepted

## Context

AstraEngine must support legacy visual novel projects, external engine packages, and modernization workflows without making import or conversion the default path. Earlier design used `RuntimeBackend`, `IRuntimeHost`, and `IServicePort` as a compatibility-specific replacement layer. That created a second runtime abstraction parallel to the native Astra runtime and duplicated concerns already covered by dynamic modules, VFS, AssetRegistry, Runtime Services facades, Runtime ECS internals, SaveService, Editor tools, MCP tools, and Cook/Release Gate.

The current architecture already has the mechanisms needed for compatibility and modernization:

- Dynamic modules and `ExtensionRegistry`.
- VFS mount providers and package readers.
- `foreign-*` AssetId schemes and external metadata.
- Runtime Services facades and RuntimeCommand.
- Runtime ECS internal system packs.
- VN Property System for configuration and serializable state.
- Editor panels, MCP tools, diagnostics, and Release Gate extensions.

## Decision

AstraEngine will not use `RuntimeBackend`, `IRuntimeHost`, or `IServicePort` for compatibility. Compatibility and legacy-game modernization are implemented as dynamic modules that register existing extension points.

Compatibility modules may provide:

- VFS mount providers for directories and archive formats.
- Foreign asset resolvers and external metadata validators.
- RuntimeCommand sources or script/timeline adapters that drive the existing Astra runtime without converting external projects into canonical Astra source files.
- Runtime ECS system packs that operate only through approved Runtime Services extension APIs and never expose EnTT types.
- SaveService extension state described by VN Property System.
- Editor inspectors, MCP diagnostics tools, Cook processors, and Release Gate checks.
- Modernization overlays for UI, input, scaling, audio bus routing, localization, font replacement, asset upscaling references, and presentation rules.

Import remains a non-goal. External original assets remain in the user's local original game directory by default and are referenced through `foreign-*` AssetId and text metadata. Cook/package must not copy external original assets unless the project has explicit authorization and configuration.

## Consequences

- Astra native projects run through the Astra runtime directly; there is no primary runtime replacement selection layer.
- Legacy compatibility does not replace the runtime. It extends VFS, AssetRegistry, Runtime Services, Editor, MCP, Cook, and Release Gate through dynamic modules.
- Compatibility modules cannot bypass Runtime Services, RuntimeCommand, VN Property System, SaveService, AssetRegistry, or Release Gate.
- Save/load stores compatibility extension state through SaveService extension slots, not private replacement-runtime blobs.
- Tests should validate module registration, mount-only behavior, foreign asset resolution, RuntimeCommand-source playback, Runtime Services facade behavior, SaveService extension snapshots, and diagnostics.

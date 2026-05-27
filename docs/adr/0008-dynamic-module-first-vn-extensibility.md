# ADR 0008: Dynamic Module First VN Extensibility

Status: Accepted

## Context

AstraEngine is now scoped as a visual-novel-focused engine and toolchain that aims for UE-level customization and extensibility within the VN domain. The engine must support native Astra projects, AI-assisted authoring, MCP tooling, asset pipeline extensions, runtime UI/audio/text customization, and compatibility or emulation modules for external VN engines.

ADR 0001 selected source-level CMake plugins for the earliest implementation because Runtime Services and editor extension points were unstable. That decision is no longer the long-term architecture target. It remains useful as an early bootstrap option for internal or experimental engine code, but project-facing extensibility should be dynamic-module-first.

## Decision

AstraEngine will use dynamic modules as the default extension model for Runtime, Editor, Developer, MCP, Asset Pipeline, AI Provider, and Compatibility capabilities.

Dynamic modules expose a narrow `AstraModule` C ABI entrypoint. The ABI boundary must not expose C++ STL containers, EnTT types, renderer or audio native handles, editor UI objects, or ownership-sensitive C++ types. A C++ SDK may wrap the ABI for plugin authors, but the stable boundary remains the C ABI.

AstraEngine will introduce:

- `ModuleManager` for discovery, dependency resolution, version checks, loading, activation, deactivation, unloading, and diagnostics.
- `ExtensionRegistry` for registering runtime service extensions, RuntimeCommand sources, ECS system packs, script functions, Story Graph node types, asset validators, cook processors, editor panels, MCP resources/tools, compatibility adapters, and AI providers.
- `PluginDescriptor` as text-first module metadata with stable IDs, API version constraints, module type, load phase, dependencies, capabilities, permissions, and platform filters.
- `VN Property System` for type IDs, property descriptors, enum metadata, default values, editor metadata, schema generation, AI-editable/read-only flags, serialization hooks, and plugin configuration validation.

AstraEngine will not introduce a full UE-style `UObject`, UHT, general Actor/Gameplay framework, or engine-wide garbage-collected object model. UE-level customization is limited to the visual novel domain: story, dialogue, stage presentation, UI, input, audio, save/load, localization, AI content, asset pipeline, editor tools, MCP tools, and compatibility modules.

## Consequences

- Source-level CMake plugins from ADR 0001 become a bootstrap/internal option, not the default public extensibility model.
- Public plugin-facing APIs must be smaller and more stable than internal C++ module APIs.
- Runtime ECS remains an internal implementation detail; dynamic modules interact through Runtime Services facades, RuntimeCommand, ExtensionRegistry, DTOs, and the VN Property System.
- Compatibility adapters such as Director, Ren'Py, KiriKiri, and NScripter support can be distributed as dynamic modules without replacing the Astra runtime.
- Packaged runtime includes only enabled runtime-safe modules. Editor, Developer, MCP debug, and authoring-only modules are excluded by default.
- Hot reload is an editor development feature. The first version only promises safe unload/reload boundaries, not arbitrary in-frame binary replacement.
- Release Gate must validate plugin descriptors, ABI compatibility, permissions, dependency closure, platform filters, and packaged-module eligibility.

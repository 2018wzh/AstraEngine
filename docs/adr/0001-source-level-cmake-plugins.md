# ADR 0001: Source-Level CMake Plugins

Status: Superseded by [ADR 0008](0008-dynamic-module-first-vn-extensibility.md)

## Context

AstraEngine was originally designed around Runtime and Editor plugins while Runtime Services and editor extension points were still early and expected to change. Promising a dynamic library ABI too early would have made those interfaces expensive to evolve.

ADR 0008 changes the long-term direction to dynamic-module-first extensibility for visual-novel-focused Runtime, Editor, Developer, MCP, Asset Pipeline, AI Provider, and Compatibility capabilities. This ADR remains as early bootstrap history and as an option for internal or experimental engine modules.

## Decision

Source-level plugins may still be used for engine core code, experimental low-level features, or extension points that have not reached ABI stability. They are no longer the default public plugin model.

## Consequences

- ADR 0008 defines `ModuleManager`, `ExtensionRegistry`, `PluginDescriptor`, `AstraModule` C ABI, and the VN Property System as the target plugin architecture.
- Source-level modules remain useful for bootstrap and internal implementation, but project-facing extensibility should be implemented as dynamic modules once the relevant extension point is public.

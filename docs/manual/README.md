# AstraEngine Manual

Status: NativeVN runtime evidence scaffold. Foundation Core, Platform, ModuleRuntime, and PropertySystem now include production Foundation gate evidence; Scene, Runtime, Asset, Media, Script, AstraVN, the example plugin, and foundation CLI workflows are implemented as foundation slices. Engine libraries are dynamic-only `Astra*` DLLs. `Samples/NativeVN` now has source asset sidecars, package manifest evidence, package launch smoke, and golden replay comparison evidence. Production Editor, AI/MCP, Legacy, real binary asset cooking, full release gates, and production backend workflows are planned.

## Overview

This manual is the user-facing documentation root for AstraEngine. It complements `docs/design`, which describes target architecture, by giving developers and creators stable places to learn workflows, APIs, systems, samples, migration, and release notes as implementation arrives.

## Key Concepts

- AstraEngine is a modular 2D engine with VN / interactive narrative as the first vertical module.
- Runtime must be able to ship without Editor.
- Core must stay free of VN, AI, Lua, renderer, editor, and legacy compatibility dependencies.
- Dynamic modules and C ABI are the default project extension boundary.
- Phase 1 provides a production Foundation gate slice for Core, Platform, ModuleRuntime, and PropertySystem, including `foundation_core_gate` CLI evidence.
- Phase 2-4 provide executable foundations for Scene, Runtime, Asset, Media, ScriptRuntimeHost, and AstraVN.
- NativeVN runtime evidence covers `validate -> cook -> package -> run --headless-smoke -> replay --compare -> inspect` for the current source-sidecar package workflow.

## Architecture

The target architecture is mapped in [design README](../design/README.md), [goals](../design/goals.md), [architecture](../design/architecture.md), and [implementation coverage](../design/implementation-coverage.md).

## Programming Guide

Start with:

- [Getting Started](getting-started/README.md)
- [Programming](programming/README.md)
- [Systems](systems/README.md)
- [API Reference](api/README.md)

## API Reference

Foundation public headers exist for Core, Platform, ModuleRuntime, PropertySystem, Scene, Runtime, Asset, Media, Script, AstraVN, and Tools. API pages index those headers, including generated dynamic-library export headers, the Phase 1 Foundation gate APIs, NativeVN package/replay evidence DTOs, and later production runtime contracts marked planned.

## Examples

Foundation/evidence samples live under `Samples/NativeVN`, `Samples/RuntimeStress`, `Samples/PackageSmoke`, and `Samples/TsuiNoSora`. `NativeVN` currently carries the redistributable runtime package/replay evidence slice, while `TsuiNoSora` is local-test-only fixture evidence. See [Samples](samples/README.md) for target workflows and current status.

## Troubleshooting

If a page describes a final release command, check whether it is listed as current evidence or target acceptance. Current validation covers CMake configure/build, `AstraPhaseTests`, the example foundation plugin load path, `astra --version`, `astra validate`, `foundation_core_gate`, dynamic engine DLL evidence, NativeVN `cook/package/run --headless-smoke/replay --compare/inspect` evidence, and `tools/doc-check.ps1`.

## Manual Sections

- [Getting Started](getting-started/README.md)
- [Programming](programming/README.md)
- [Systems](systems/README.md)
- [API Reference](api/README.md)
- [Editor](editor/README.md)
- [Samples](samples/README.md)
- [Migration](migration/README.md)
- [Release Notes](release-notes/README.md)
- [Concepts](concepts/README.md)

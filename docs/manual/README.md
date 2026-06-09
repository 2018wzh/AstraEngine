# AstraEngine Manual

Status: Phase 4 manual scaffold. Foundation Core, Platform, ModuleRuntime, and PropertySystem now include production Foundation gate evidence; Scene, Runtime, Asset, Media, Script, AstraVN, the example plugin, and foundation CLI workflows are implemented as foundation slices. Production Editor, AI/MCP, Legacy, full package/replay, and production backend workflows are planned.

## Overview

This manual is the user-facing documentation root for AstraEngine. It complements `docs/design`, which describes target architecture, by giving developers and creators stable places to learn workflows, APIs, systems, samples, migration, and release notes as implementation arrives.

## Key Concepts

- AstraEngine is a modular 2D engine with VN / interactive narrative as the first vertical module.
- Runtime must be able to ship without Editor.
- Core must stay free of VN, AI, Lua, renderer, editor, and legacy compatibility dependencies.
- Dynamic modules and C ABI are the default project extension boundary.
- Phase 1 provides a production Foundation gate slice for Core, Platform, ModuleRuntime, and PropertySystem, including `foundation_core_gate` CLI evidence.
- Phase 2-4 provide executable foundations for Scene, Runtime, Asset, Media, ScriptRuntimeHost, and AstraVN.

## Architecture

The target architecture is mapped in [design README](../design/README.md), [goals](../design/goals.md), [architecture](../design/architecture.md), and [implementation coverage](../design/implementation-coverage.md).

## Programming Guide

Start with:

- [Getting Started](getting-started/README.md)
- [Programming](programming/README.md)
- [Systems](systems/README.md)
- [API Reference](api/README.md)

## API Reference

Foundation public headers exist for Core, Platform, ModuleRuntime, PropertySystem, Scene, Runtime, Asset, Media, Script, and AstraVN. API pages index those headers, including the Phase 1 Foundation gate APIs, and keep later production runtime contracts marked planned.

## Examples

Foundation samples live under `Samples/NativeVN`, `Samples/RuntimeStress`, and `Samples/PackageSmoke`. See [Samples](samples/README.md) for target workflows and current status.

## Troubleshooting

If a page describes a final release command such as `astra replay`, treat it as target acceptance unless the page explicitly marks it implemented. Current validation covers CMake configure/build, `Astra_Phase1Tests`, the example foundation plugin load path, `astra --version`, `astra validate`, `foundation_core_gate`, foundation `cook/package/run --headless-smoke`, Phase 4 NativeVN headless evidence, and `tools/doc-check.ps1`.

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

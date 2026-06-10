# Systems

Status: NativeVN runtime evidence scaffold. Foundation diagnostics, platform services, module loading, property schema generation, dynamic engine DLL evidence, Scene, Runtime, Save/Replay snapshots, Asset sidecars/VFS/registry/dependency graph, DDC metadata DTOs, local DDC artifact execution/reuse/corruption recovery, embedded package payloads, PackageReader random-access/chunked-read/mount evidence, package/cook/payload integrity diagnostics, NativeVN cook/package/replay reports, Media provider release-gate checks, mature media backend capability reports, image metadata decode smoke, Media headless verification, ScriptRuntimeHost, Lua/Native DSL bridge, and AstraVN headless session evidence have executable implementations; production binary Asset/Media/Script/AstraVN/full Release Gate systems are planned.

## Overview

This section will document operational systems: Asset Pipeline, Cook/Package, Save/Replay, Renderer2D, Text/Font, Audio, FilterGraph, Hot Reload, Diagnostics, Release Gate, and Observability.

## Key Concepts

- Systems must expose machine-readable diagnostics.
- Cooked, cached, generated, and packaged outputs are not canonical source.
- Headless verification is required for CI and release checks.
- Runtime systems must not depend on Editor.

## Architecture

Primary design references:

- [Asset Pipeline](../../design/asset-pipeline.md)
- [Content and Assets](../../design/content-and-assets.md)
- [Media Runtime](../../design/media-runtime.md)
- [Runtime Core](../../design/runtime-core.md)
- [Script and Presentation](../../design/script-and-presentation.md)
- [Tools / Release / Observability](../../design/tools-release-observability.md)

## Programming Guide

Implemented foundation system pages:

- [Platform Backend Porting](platform/backend-porting.md)

Runtime foundation pages:

- [Foundation Save/Replay Guide](../programming/runtime/save-replay.md)

Media and Script/AstraVN foundation pages:

- [Media Foundation](media/README.md)
- [Headless Renderer](media/headless-renderer.md)
- [Audio And Text DTOs](media/audio-text-dtos.md)
- [FilterGraph Foundation](media/filtergraph.md)
- [Script Foundation](../programming/script/README.md)
- [AstraVN Foundation](../programming/astravn/README.md)

Future system pages should include inputs, outputs, diagnostics, release-gate checks, and headless test evidence.

## API Reference

Foundation system APIs are implemented for diagnostics, platform services, module registries, property schema generation, Scene, Runtime, Save containers, replay hash comparison, VFS, AssetRegistry scans, dependency graph reports, cook/package manifests, DDC metadata entries, local DDC artifact execution, embedded package payload entries, PackageReader bytes/text/chunked reads and mount DTOs, package reader integrity diagnostics, PresentationCommand DTOs, headless Renderer2D capture, TextLayout request DTOs, Audio command DTOs, FilterProfile validation, media backend capability reports, image metadata inspect reports, foundation media provider release-gate reports, ScriptRuntimeHost, ScriptEventBridge, and VnSession. Planned references include production media-transform importer/cooker execution, real Renderer2D provider, TextLayout provider, Audio provider, production Replay stream, Script debugger, Graph/Timeline, and production AstraVN package launch.

## Examples

Current examples include validating asset sidecars in tests, NativeVN AssetRegistry/dependency graph evidence, DDC artifact emission/reuse/corruption recovery, media backend capability evidence through `astra validate`, headless media hash verification through `astra run --headless-smoke`, NativeVN package manifest generation, embedded payload read/chunk/mount smoke, package integrity checks, package launch smoke, and golden replay comparison. Planned examples include production binary media cooking, real media execution backend verification, and richer replay mismatch localization.

## Troubleshooting

- Treat full binary asset/media release commands as target commands until those systems exist; current NativeVN `validate`, `cook`, `package`, `run --headless-smoke`, `replay --compare`, and `inspect` are evidence-slice workflows, not proof of production media/backend completion.
- Release evidence must be current command output, not design intent.

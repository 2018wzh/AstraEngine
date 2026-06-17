# Systems

Status: NativeVN runtime evidence plus Phase 6 Asset Pipeline. Foundation diagnostics, platform services, module loading, property schema generation, dynamic engine DLL evidence, Scene, Runtime, Save/Replay snapshots, Asset sidecars/VFS/registry/dependency graph, production import/cook/DDC, binary `.astrapkg`, PackageReader random-access/chunked-read/mount evidence, Asset Release Gate reports, package/cook/payload integrity diagnostics, NativeVN cook/package/replay reports, Media provider release-gate checks, mature media backend capability reports, image metadata decode smoke, Media headless verification, ScriptRuntimeHost, Lua/Native DSL bridge, and AstraVN headless session evidence have executable implementations; production Media/Script/AstraVN/Editor/AI systems are planned.

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

Foundation and Phase 6 system APIs are implemented for diagnostics, platform services, module registries, property schema generation, Scene, Runtime, Save containers, replay hash comparison, VFS, AssetRegistry scans, dependency graph reports, importer/cooker descriptors, cook/package manifests, local DDC entries, binary package payload entries, PackageReader bytes/text/chunked reads and mount DTOs, package reader integrity diagnostics, Asset Release Gate reports, hot reload rollback DTOs, PresentationCommand DTOs, headless Renderer2D capture, TextLayout request DTOs, Audio command DTOs, FilterProfile validation, media backend capability reports, image metadata inspect reports, foundation media provider release-gate reports, ScriptRuntimeHost, ScriptEventBridge, and VnSession. Planned references include real Renderer2D provider execution, TextLayout provider execution, Audio provider execution, Script debugger, Graph/Timeline, and production AstraVN authoring workflows.

## Examples

Current examples include validating asset sidecars in tests, NativeVN AssetRegistry/dependency graph evidence, DDC artifact emission/reuse/corruption recovery, media backend capability and Phase 7 provider evidence through `astra validate`, headless media hash verification through `astra run --headless-smoke`, NativeVN binary package generation, zstd payload read/chunk/mount smoke, Asset Release Gate checks, package manifest hash/provider feature hash save-replay evidence, package integrity checks, package launch smoke, and golden replay comparison. Planned examples include deeper per-driver visual/audio replay diff.

## Troubleshooting

- Current NativeVN `validate`, `cook`, `package`, `run --headless-smoke`, `replay --compare`, and `inspect` prove the Phase 6 package workflow plus Phase 7 media provider/decode/timeline/filter evidence; per-driver visual/audio diff remains later hardening.
- Release evidence must be current command output, not design intent.

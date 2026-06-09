# Systems

Status: Phase 4 scaffold. Foundation diagnostics, platform services, module loading, property schema generation, Scene, Runtime, Save/Replay snapshots, Asset sidecars/VFS/registry, Media provider release-gate checks, Media headless verification, ScriptRuntimeHost, Lua/Native DSL bridge, and AstraVN headless session evidence have executable implementations; production Asset/Media/Script/AstraVN/full Release Gate systems are planned.

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

Foundation system APIs are implemented for diagnostics, platform services, module registries, property schema generation, Scene, Runtime, Save snapshots, replay hash smoke, VFS, AssetRegistry scans, PresentationCommand DTOs, headless Renderer2D capture, TextLayout request DTOs, Audio command DTOs, FilterProfile validation, foundation media provider release-gate reports, ScriptRuntimeHost, ScriptEventBridge, and VnSession. Planned references include production importer/cooker/package reader, real Renderer2D provider, TextLayout provider, Audio provider, production Save container, production Replay stream, Script debugger, Graph/Timeline, and production AstraVN package launch.

## Examples

Current examples include validating an asset sidecar in tests, headless media hash verification through `astra run --headless-smoke`, and NativeVN Phase 4 script/VN evidence. Planned examples include production cooking and replay mismatch reporting.

## Troubleshooting

- Treat `astra replay` and full asset/media release commands as target commands until those systems exist; current `astra validate`, `cook`, `package`, and `run --headless-smoke` remain foundation-only.
- Release evidence must be current command output, not design intent.

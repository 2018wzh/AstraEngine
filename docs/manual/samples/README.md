# Samples

Status: NativeVN runtime evidence scaffold. `Samples/NativeVN`, `Samples/RuntimeStress`, and `Samples/PackageSmoke` exist as foundation descriptors; `NativeVN` now has a headless playable Script/AstraVN slice plus source asset sidecars, AssetRegistry/dependency graph evidence, local DDC artifact/corruption recovery evidence, embedded package payload read/mount evidence, package manifest integrity evidence, package launch smoke, and golden replay comparison. Full production content samples remain planned.

## Overview

Samples will provide acceptance evidence for runtime, tools, release gate, documentation, and CI. They are not decorative demos.

## Key Concepts

- `NativeVN` is the final UE-class acceptance sample and currently provides Script/AstraVN, package, and replay evidence for the source-sidecar runtime slice.
- `PackageSmoke` currently proves the foundation headless platform plus example module load/unload and Phase 3 media smoke path; later it will prove packaged runtime has no Editor dependency.
- `RuntimeStress`, `ScriptParity`, `MediaBackend`, `AIIntentSafety`, `CreatorWorkflow`, `CustomizationPlugin`, and `CompatMockExpansion` each cover a specific risk.
- Evidence must come from current local or CI command output.

## Architecture

Sample requirements are specified in [Samples and Test Matrix](../../design/samples-and-test-matrix.md) and [Implementation Coverage](../../design/implementation-coverage.md).

## Programming Guide

Current foundation sample descriptors live in `Samples/*/astra.sample.yaml`. `Samples/NativeVN/Content/Scripts` contains the Native DSL and Lua parity sources; `Samples/NativeVN/Content` also contains source asset sidecars for background, character, voice, music, filter, and script assets. Later content samples must add real binary media sources, expected visual/audio output, troubleshooting, release checklist, and production evidence artifacts.

## API Reference

Foundation descriptors use `schema: astra.sample.v1` and `foundation_only: true`. `NativeVN` currently uses `phase: 4` and source asset sidecars with `schema: astra.asset.sidecar.v1`; future production descriptors should link to project template, real binary asset sources, package manifest, replay, and diagnostics schemas.

## Examples

Runnable foundation commands include:

```powershell
astra validate Samples/PackageSmoke --strict
astra package Samples/PackageSmoke --profile development
astra run Samples/PackageSmoke --headless-smoke
astra validate Samples/NativeVN --strict
astra run Samples/NativeVN --headless-smoke
```

NativeVN runtime evidence commands include:

```powershell
astra validate Samples/NativeVN --strict --json
astra cook Samples/NativeVN --config Release
astra package Samples/NativeVN --profile deterministic
astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke
astra replay build/Saved/Replays/NativeVNGolden.replay --compare
astra inspect build/Saved/Packages/NativeVN.astrapkg
```

These commands prove the current source-sidecar runtime evidence slice, including local DDC artifact writes, DDC corruption recovery, embedded package payload random/chunked reads, package mount DTOs, and package/cook/payload manifest hash checks. They do not yet prove real binary media cooking, a production media backend, binary package streaming at scale, Editor workflows, AI/MCP, Legacy, or the final full release gate.

## Troubleshooting

- Do not restore deleted `AstraGame` or `MinimalVN` flows.
- Do not count a design document as sample evidence.
- Keep legacy compatibility samples in the expansion track.

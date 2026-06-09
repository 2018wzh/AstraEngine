# Samples

Status: Phase 4 scaffold. `Samples/NativeVN`, `Samples/RuntimeStress`, and `Samples/PackageSmoke` exist as foundation-only descriptors; `NativeVN` now has a Phase 4 headless playable Script/AstraVN slice. Full production content samples remain planned.

## Overview

Samples will provide acceptance evidence for runtime, tools, release gate, documentation, and CI. They are not decorative demos.

## Key Concepts

- `NativeVN` is the final UE-class acceptance sample and currently provides Phase 4 Script/AstraVN foundation evidence.
- `PackageSmoke` currently proves the foundation headless platform plus example module load/unload and Phase 3 media smoke path; later it will prove packaged runtime has no Editor dependency.
- `RuntimeStress`, `ScriptParity`, `MediaBackend`, `AIIntentSafety`, `CreatorWorkflow`, `CustomizationPlugin`, and `CompatMockExpansion` each cover a specific risk.
- Evidence must come from current local or CI command output.

## Architecture

Sample requirements are specified in [Samples and Test Matrix](../../design/samples-and-test-matrix.md) and [Implementation Coverage](../../design/implementation-coverage.md).

## Programming Guide

Current foundation sample descriptors live in `Samples/*/astra.sample.yaml`. `Samples/NativeVN/Content/Scripts` contains the Phase 4 Native DSL and Lua parity sources. Later content samples must add expected output, troubleshooting, release checklist, and evidence artifacts.

## API Reference

Foundation descriptors use `schema: astra.sample.v1` and `foundation_only: true`. `NativeVN` currently uses `phase: 4`; future production descriptors should link to project template, asset sidecar, package manifest, replay, and diagnostics schemas.

## Examples

Runnable foundation commands include:

```powershell
astra validate Samples/PackageSmoke --strict
astra package Samples/PackageSmoke --profile development
astra run Samples/PackageSmoke --headless-smoke
astra validate Samples/NativeVN --strict
astra run Samples/NativeVN --headless-smoke
```

Target final acceptance commands include:

```powershell
astra validate Samples/NativeVN
astra cook Samples/NativeVN --config Release
astra package Samples/NativeVN --deterministic
astra run Saved/Cooked/NativeVN --headless-smoke
astra replay Saved/Replays/NativeVNGolden.replay --compare
astra inspect Saved/Packages/NativeVN.astrapkg
```

The final acceptance commands are not implemented by Phase 4 unless explicitly listed above.

## Troubleshooting

- Do not restore deleted `AstraGame` or `MinimalVN` flows.
- Do not count a design document as sample evidence.
- Keep legacy compatibility samples in the expansion track.

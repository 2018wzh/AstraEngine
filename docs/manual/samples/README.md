# Samples

Status: NativeVN playable v1 plus local TsuiNoSora playable fixture. `Samples/NativeVN`, `Samples/RuntimeStress`, `Samples/PackageSmoke`, and `Samples/TsuiNoSora` exist as foundation descriptors; `NativeVN` is the redistributable playable acceptance sample, while `TsuiNoSora` is a local-test-only sample copied from a curated Artemis resource subset for real PNG/OGG/font, UI/system, save/load, replay, and inspect evidence.

## Overview

Samples will provide acceptance evidence for runtime, tools, release gate, documentation, and CI. They are not decorative demos.

## Key Concepts

- `NativeVN` is the final UE-class acceptance sample and currently provides generated redistributable PNG/OGG fixture media, Script/AstraVN, package, playable VN state, save/load, and replay evidence.
- `TsuiNoSora` is a local verification fixture using copied test resources marked `local_test_only`; it includes a curated Aya route, Artemis-style UI/system metadata, backlog, save/load slots, config state, and system SE evidence, and it must not be treated as redistributable sample content.
- `PackageSmoke` currently proves the foundation headless platform plus example module load/unload and Phase 3 media smoke path; later it will prove packaged runtime has no Editor dependency.
- `RuntimeStress`, `ScriptParity`, `MediaBackend`, `AIIntentSafety`, `CreatorWorkflow`, `CustomizationPlugin`, and `CompatMockExpansion` each cover a specific risk.
- Evidence must come from current local or CI command output.

## Architecture

Sample requirements are specified in [Samples and Test Matrix](../../design/samples-and-test-matrix.md) and [Implementation Coverage](../../design/implementation-coverage.md).

## Programming Guide

Current sample descriptors live in `Samples/*/astra.sample.yaml`. `Samples/NativeVN/Content` contains generated redistributable PNG/OGG fixture media plus Native DSL/Lua parity sources. `Samples/TsuiNoSora/Content` contains copied local fixture PNG, OGG, font, UI/system, filter, and script sidecars for runtime verification.

## API Reference

Descriptors use `schema: astra.sample.v1` and `foundation_only: true` for the current Phase 1-6 foundation/evidence slices. `NativeVN` uses redistributable generated fixtures; `TsuiNoSora` uses `local_test_only` fixture sidecars.

## Examples

Runnable foundation commands include:

```powershell
astra validate Samples/PackageSmoke --strict
astra package Samples/PackageSmoke --profile development
astra run Samples/PackageSmoke --headless-smoke
astra validate Samples/NativeVN --strict
astra run Samples/NativeVN --headless-smoke
astra validate Samples/TsuiNoSora --strict --json
astra run build/Saved/Packages/TsuiNoSora.astrapkg --headless-smoke --json
```

NativeVN runtime evidence commands include:

```powershell
astra validate Samples/NativeVN --strict --json
astra cook Samples/NativeVN --config Release
astra package Samples/NativeVN --profile deterministic
astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --save-out build/Saved/Saves/NativeVN.save.json --json
astra run build/Saved/Packages/NativeVN.astrapkg --load build/Saved/Saves/NativeVN.save.json --headless-smoke --json
astra run build/Saved/Packages/NativeVN.astrapkg --windowed-smoke --scripted-input Samples/NativeVN/Input/golden.yaml --auto-close --json
astra replay build/Saved/Replays/NativeVNGolden.replay --compare
astra inspect build/Saved/Packages/NativeVN.astrapkg
```

These commands prove the current Phase 6 playable slice, including binary `.astrapkg` packaging, zstd payloads, generated/copy fixture media decode evidence, SDL/headless RGBA image and HarfBuzz/FreeType glyph primitive present evidence sourced from package payloads for `.astrapkg` runs, playable VN state, UI/system state, local DDC artifact writes/reuse/corruption recovery, PackageReader random/chunked reads, read-only package mount DTOs, package manifest hash/provider feature hash save-replay evidence, replay mismatch localization, Asset Release Gate evidence, and package/cook/payload manifest hash checks. They do not yet prove real media execution backends, Editor workflows, AI/MCP, Legacy, or final UE-class acceptance.

TsuiNoSora local fixture commands include:

```powershell
astra validate Samples/TsuiNoSora --strict --json
astra cook Samples/TsuiNoSora --config Debug --json
astra package Samples/TsuiNoSora --profile development --json
astra run build/Saved/Packages/TsuiNoSora.astrapkg --headless-smoke --json
astra run build/Saved/Packages/TsuiNoSora.astrapkg --headless-smoke --save-out build/Saved/Saves/TsuiNoSora.save.json --json
astra run build/Saved/Packages/TsuiNoSora.astrapkg --load build/Saved/Saves/TsuiNoSora.save.json --headless-smoke --json
astra run build/Saved/Packages/TsuiNoSora.astrapkg --windowed-smoke --scripted-input Samples/TsuiNoSora/Input/golden.yaml --auto-close --json
astra replay build/Saved/Replays/TsuiNoSoraGolden.replay --compare --json
astra inspect build/Saved/Packages/TsuiNoSora.astrapkg --json
```

## Troubleshooting

- Do not restore deleted `AstraGame` or `MinimalVN` flows.
- Do not count a design document as sample evidence.
- Keep legacy compatibility samples in the expansion track.

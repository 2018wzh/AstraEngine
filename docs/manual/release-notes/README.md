# Release Notes

Status: Phase 4 release notes index.

## Overview

Release notes record milestone changes, breaking changes, verification commands, and known gaps. They must be updated with every milestone.

## Key Concepts

- Release notes separate implemented behavior from target architecture.
- Verification commands must be commands that were actually run.
- Breaking architecture or ABI changes must point to related design or ADR updates.

## Architecture

Release evidence requirements are described in [Tools / Release / Observability](../../design/tools-release-observability.md), [Samples and Test Matrix](../../design/samples-and-test-matrix.md), and [Implementation Coverage](../../design/implementation-coverage.md).

## Programming Guide

Each milestone note should include:

- Summary.
- Breaking changes.
- Added or changed docs/manual pages.
- Build/test/doc-check commands run.
- Known planned systems not implemented yet.

## API Reference

Phase 4 adds public foundation APIs for Script and AstraVN in addition to the Phase 1/2/3 foundation APIs. The current worktree also hardens Phase 1 Core/Platform/ModuleRuntime/PropertySystem with Foundation gate APIs. Future release notes should list added, changed, deprecated, and removed public APIs.

## Examples

Foundation verification:

```powershell
cmake -S . -B build
cmake --build build --config Debug
ctest --test-dir build -C Debug --output-on-failure
powershell -NoProfile -ExecutionPolicy Bypass -File tools/doc-check.ps1
build\Bin\astra.exe --version
build\Bin\astra.exe doc-check
build\Bin\astra.exe validate Samples\PackageSmoke --strict
build\Bin\astra.exe validate Samples\NativeVN --strict --json
build\Bin\astra.exe validate . --strict --json
build\Bin\astra.exe package Samples\PackageSmoke --profile development
build\Bin\astra.exe run Samples\PackageSmoke --headless-smoke --json
build\Bin\astra.exe run Samples\NativeVN --headless-smoke --json
```

Phase 1 Foundation gate hardening:

- `Astra_Core`: diagnostic code registry, release policy, `FoundationGateReport`, release config resolve/hash, and unknown-field migration policy evidence.
- `Astra_Platform`: opaque `DynamicLibraryHandle`, file-watch polling, pending thread tags, and crash capture context.
- `Astra_ModuleRuntime`: service resolve audit, engine module slot policy validation, and module release-gate report with plugin binary SHA-256.
- `Astra_PropertySystem`: nested struct/array/map/tagged union schema generation, schema version graph validation, write policy, and release-sensitive diff/audit output.
- CLI validate for repository roots emits `foundation_core_gate` with registered diagnostic-code gate, release config hash, Property evidence, and module release-gate evidence.

Phase 4 foundation additions:

- `Astra_Asset`: asset URI/ID parsing, VFS mounts, sidecar validation, registry scan, dependency diagnostics, descriptor DTO validation, and watch invalidation records.
- `Astra_Media`: PresentationCommand, RenderGraph/text/audio/filter DTOs, FilterProfile validation/application, Renderer2D/TextLayout/Audio foundation provider descriptors, media release-gate foundation validation, HeadlessRenderer2D hashes, and SDL renderer factory compile-path stub.
- `Astra_Script`: `ScriptRuntimeHost`, Native DSL parser, Lua provider via `sol2`, shared command stream, diagnostics, debug-symbol DTOs, `ScriptSnapshot`, and `ScriptEventBridge`.
- `Astra_AstraVN`: VN event schemas, preset actors/components/state machines, `VnSession`, `VnSessionSnapshot`, headless presentation capture, and save/restore evidence.
- CLI validate/headless smoke includes `foundation_core_gate`, Phase 3 Asset/Media/FilterGraph hash evidence, media provider release-gate foundation evidence, and Phase 4 NativeVN Script/AstraVN evidence.

Known gaps: CLI still handles foundation validation/package/headless smoke only. Real asset cooking, package launch, replay command, production Media backend/provider replacement, full Script debugger/hot reload/Graph/Timeline, production AstraVN, Editor, AI/MCP, and Legacy remain planned.

## Troubleshooting

- Do not use target acceptance commands as proof until the relevant tool exists.
- If CI skips a planned system, call that out as a known gap instead of implying it passed.

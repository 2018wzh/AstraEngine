# Release Notes

Status: NativeVN runtime evidence release notes index.

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

Phase 4 adds public foundation APIs for Script and AstraVN in addition to the Phase 1/2/3 foundation APIs. The current worktree also switches engine libraries to dynamic-only `Astra*` DLLs and adds NativeVN package/replay evidence DTOs. Future release notes should list added, changed, deprecated, and removed public APIs.

## Examples

Foundation verification:

```powershell
cmake -S . -B build
cmake --build build --config Debug
ctest --test-dir build -C Debug --output-on-failure
build\Bin\astra.exe --version
build\Bin\astra.exe doc-check
build\Bin\astra.exe validate Samples\PackageSmoke --strict
build\Bin\astra.exe validate Samples\NativeVN --strict --json
build\Bin\astra.exe validate . --strict --json
build\Bin\astra.exe package Samples\PackageSmoke --profile development
build\Bin\astra.exe run Samples\PackageSmoke --headless-smoke --json
build\Bin\astra.exe run Samples\NativeVN --headless-smoke --json
build\Bin\astra.exe cook Samples\NativeVN --config Release --json
build\Bin\astra.exe package Samples\NativeVN --profile deterministic --json
build\Bin\astra.exe run build\Saved\Packages\NativeVN.astrapkg --headless-smoke --json
build\Bin\astra.exe replay build\Saved\Replays\NativeVNGolden.replay --compare --json
build\Bin\astra.exe inspect build\Saved\Packages\NativeVN.astrapkg --json
```

Dynamic linking changes:

- `astra_add_library` builds `Astra*` engine/runtime/tool libraries as shared libraries.
- Per-module export headers are generated under `Astra/<Module>/Export.hpp`.
- Plugin targets remain `MODULE` binaries using the C ABI entrypoint.
- CLI release evidence reports engine DLL SHA-256 hashes from the runtime output directory.

Phase 1 Foundation gate hardening:

- `AstraCore`: diagnostic code registry, release policy, `FoundationGateReport`, release config resolve/hash, and unknown-field migration policy evidence.
- `AstraPlatform`: opaque `DynamicLibraryHandle`, file-watch polling, pending thread tags, and crash capture context.
- `AstraModuleRuntime`: service resolve audit, engine module slot policy validation, and module release-gate report with plugin binary SHA-256.
- `AstraPropertySystem`: nested struct/array/map/tagged union schema generation, schema version graph validation, write policy, and release-sensitive diff/audit output.
- CLI validate for repository roots emits `foundation_core_gate` with registered diagnostic-code gate, release config hash, Property evidence, and module release-gate evidence.

Phase 4 foundation additions:

- `AstraAsset`: asset URI/ID parsing, VFS mounts, sidecar validation, registry scan, dependency diagnostics, descriptor DTO validation, and watch invalidation records.
- `AstraMedia`: PresentationCommand, RenderGraph/text/audio/filter DTOs, FilterProfile validation/application, Renderer2D/TextLayout/Audio foundation provider descriptors, media release-gate foundation validation, HeadlessRenderer2D hashes, and SDL renderer factory compile-path stub.
- `AstraScript`: `ScriptRuntimeHost`, Native DSL parser, Lua provider via `sol2`, shared command stream, diagnostics, debug-symbol DTOs, `ScriptSnapshot`, and `ScriptEventBridge`.
- `AstraVN`: VN event schemas, preset actors/components/state machines, `VnSession`, `VnSessionSnapshot`, headless presentation capture, and save/restore evidence.
- CLI validate/headless smoke includes `foundation_core_gate`, Phase 3 Asset/Media/FilterGraph hash evidence, media provider release-gate foundation evidence, and Phase 4 NativeVN Script/AstraVN evidence.
- NativeVN package/replay evidence includes source asset sidecars, AssetRegistry/dependency graph reports, cook manifests, image cook artifact metadata, local DDC artifact writes, DDC reuse/rebuild/corruption recovery reports, deterministic package manifests, embedded package payload tables, PackageReader random-access/chunked-read/mount smoke, package/cook/payload hash integrity diagnostics, mature media backend capability reports, libpng image metadata decode smoke, engine/plugin DLL hash evidence, package launch smoke, and golden replay hash comparison.

Phase 6 asset pipeline additions:

- `AstraAsset`: `ImportRequest`, `ImporterDescriptor`, `IAssetImporter`, `CookRequest`, `CookArtifactDescriptor`, `DdcKey`, `DdcCleanReport`, `ICookProcessor`, `PackagePayloadRef`, `PackageMountPolicy`, `AssetReleaseGateReport`, `HotReloadTransaction`, built-in importers/cook processors, DDC reuse/rebuild/clean/corruption recovery, and provider feature hash helpers.
- `.astrapkg`: binary `ASTRAP6\0` container with embedded canonical JSON manifest, zstd-compressed payload table, payload offsets, compressed sizes, SHA-256 validation, random-access reads, chunked reads, text reads, and read-only package mount policy.
- CLI: `astra import`, production `cook/package/inspect`, package-only `run`, save/replay package manifest hash/provider feature hash evidence, and replay mismatch localization for current frame/record/source-object/package hash reports.
- Tests: import validation, DDC reuse/rebuild/corruption recovery, release gate blockers, binary package hash validation, zstd payload reads, chunked reads, and hot reload rollback DTO coverage.

Known gaps: per-driver visual/audio diff, full Script debugger/hot reload/Graph/Timeline, production AstraVN authoring surface, Editor, AI/MCP, and Legacy remain planned. Phase 7 media provider/decode/timeline/filter evidence is implemented through DTO-safe provider contracts and headless deterministic hashes.

Production logging stage additions:

- Dependency: `spdlog` is now a required private `AstraCore` dependency through vcpkg/CMake.
- `AstraCore`: `LogLevel`, `LogEvent` schema `astra.log.event.v1`, `LogConfig`, process default `Logger`, async rotating JSONL file output, console output, memory capture for tests, recent-log ring, and `LogDiagnostic()` mirroring.
- CLI: global/subcommand `--log-dir`, `--log-file`, `--log-level`, `--log-async`, and `--log-sync`; default tools logging writes console plus `build/Saved/Logs/astra.log.jsonl`.
- Runtime coverage: representative lifecycle/operation channels now cover Tools, Platform, ModuleRuntime, Asset cook/package, Runtime events/tick/save/load, Media backend/render/decode, Script compile/execute, and AstraVN session/snapshot.
- Tests: Core logging JSONL/memory/diagnostic mirroring coverage and `AstraCliValidateNativeVNLogs` CLI coverage.
- Still planned: full trace capture/export, profiler backend export, and production crash bundle generation.

TsuiNoSora local-data port additions:

- `Samples/TsuiNoSora` is a local-data AstraVN conversion sample.
- TsuiNoSora-specific conversion, patching, and Director-era container probing live only under `Samples/TsuiNoSora/Tools`.
- Engine tools now accept Phase 8 AstraVN playable samples described by `runtime: astra_vn` or `playable:` instead of hard-coding only `NativeVN`.
- Engine tools now provide `astra package --shipping` and `astra play` so Shipping bundles use a production launcher instead of smoke QA commands.
- SDL input now fills the existing `InputSnapshot` DTO for generic packaged VN interaction.
- Original data and generated commercial `Content/` stay untracked.

## Troubleshooting

- Do not use target acceptance commands as proof until the relevant tool exists.
- If CI skips a planned system, call that out as a known gap instead of implying it passed.

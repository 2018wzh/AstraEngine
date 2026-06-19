# AstraEmu Production Checklist

Status: Standalone expansion audit ledger / AstraEmu not implemented  
Last audited: 2026-06-19

Legend:

- [x] Code-backed complete in the current source tree, with automated test or CLI evidence.
- [~] Partial reusable AstraEngine foundation exists, but no AstraEmu-specific implementation.
- [ ] Planned or not implemented.

Rule: AstraEmu is a post-native-parity standalone toolkit. It must not become a prerequisite for NativeVN authoring, packaged runtime launch, or EngineCore acceptance.

## Boundary And Ordering

- [~] EMU-BOUND-001 - AstraEmu is documented as a standalone post-parity toolkit, not a NativeVN creation path. Design: `goals.md`, `architecture.md`, `compatibility-layer.md`, `legacy-compatibility-contract.md`. Code: none for AstraEmu. Evidence: `astra doc-check`. Gap: no toolkit implementation.
- [x] EMU-BOUND-002 - Current native runtime samples build and package without an AstraEmu target or module. Design: `samples-and-test-matrix.md`. Code: `CMakeLists.txt`, `Engine/Tests/CMakeLists.txt`, no `Engine/Runtime/AstraEmu` target. Evidence: `AstraCliPackageNativeVN`, `AstraCliRunNativeVNPackage`. Gap: once AstraEmu exists, add explicit negative dependency tests.
- [ ] EMU-BOUND-003 - AstraEmu distribution mode is separate from AstraEditor and NativeVN package flow. Design: `tools-release-observability.md`, `compatibility-layer.md`. Code: none. Evidence: none. Gap: standalone toolkit executable/package profile.

## Manager And Host Facade

- [ ] EMU-MGR-001 - AstraEmu Manager thin host facade exists over ModuleRuntime, ServiceRegistry, Asset VFS, PackageReader, RuntimeWorld, Save/Replay, Media providers, and FilterGraph. Design: `legacy-compatibility-contract.md` section 2. Code: none. Evidence: none. Gap: implement manager without parallel runtime, filesystem, package, renderer, audio, or save systems.
- [ ] EMU-MGR-002 - Manager owns window/input/config/backend selection for toolkit mode only. Design: `compatibility-layer.md` RetroArch-style structure. Code: none. Evidence: none. Gap: toolkit lifecycle and CLI/UI entry.
- [ ] EMU-MGR-003 - Manager exposes Runtime Inspector for probe result, core descriptor, mount status, VM PC/state summary, unsupported tag/API coverage, TextCapture, translation audit, backend capability, and save-state summary. Design: `compatibility-layer.md` section 10. Code: none. Evidence: none. Gap: inspector DTOs and UI/CLI.
- [ ] EMU-MGR-004 - Manager can remount local root, switch core, cold-swap core, inspect read-only member, toggle enhancement profile, toggle translation provider, and export compatibility report. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: command surface and diagnostics.

## Provider Descriptors, Slots, And Module Lifecycle

- [ ] EMU-PROV-001 - Compat Core provider descriptor follows `astra.provider.descriptor.v1` with `contract: ICompatRuntimeProvider`. Design: `legacy-compatibility-contract.md` section 3. Code: none. Evidence: none. Gap: descriptor schema and validator registration for compat providers.
- [ ] EMU-PROV-002 - EngineModuleSlot `astra.emu.compat_core` selects the active Compat Core explicitly. Design: `legacy-compatibility-contract.md`. Code: none; generic `EngineModuleRegistry` exists. Evidence: no AstraEmu test. Gap: slot registration and selection policy.
- [ ] EMU-PROV-003 - Translation provider slot `astra.emu.translation_provider` is separate from Compat Core. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: provider descriptor, capability set, permissions, audit.
- [ ] EMU-PROV-004 - AstraEmu reuses renderer/text/audio/filter/decode slots instead of registering private media backends. Design: `legacy-compatibility-contract.md`. Code: none for AstraEmu; media slots exist in `Media.hpp`. Evidence: media tests only. Gap: toolkit policy and provider selection.
- [ ] EMU-PROV-005 - Compat Core modules follow ModuleRuntime lifecycle: discover, validate, load, initialize, activate, deactivate, shutdown, unload. Design: `extension-and-module-system.md`, `legacy-compatibility-contract.md`. Code: none for Compat Core; generic ModuleRuntime exists. Evidence: Phase1Example only. Gap: compat module sample and failure tests.
- [ ] EMU-PROV-006 - Release Gate validates Compat Core descriptor, binary hash, ABI compatibility, required services, permissions, packaged/toolkit eligibility, and dependency closure. Design: `provider-contracts.md`, `release-gate-observability-contract.md`. Code: none for AstraEmu. Evidence: none. Gap: AstraEmuToolkit release-gate scenarios.

## Content Probe, Mounts, And Foreign Source Policy

- [ ] EMU-PROBE-001 - CompatContentProbe scans user local roots and returns engine family, version, confidence, entry candidates, resource roots, diagnostics. Design: `legacy-compatibility-contract.md` section 4. Code: none. Evidence: none. Gap: probe request/result DTOs and mock probe.
- [ ] EMU-PROBE-002 - Local foreign directories mount read-only through `foreign-*:/` VFS schemes. Design: `compatibility-layer.md`, `legacy-compatibility-contract.md`. Code: none for AstraEmu; generic VFS exists in `AstraAsset`. Evidence: asset VFS tests only. Gap: foreign mount policy and write-block diagnostics.
- [ ] EMU-PROBE-003 - PackageReader read-only package mounts are reused for indexed packaged content. Design: `legacy-compatibility-contract.md`. Code: none for AstraEmu; `PackageReader` exists. Evidence: package reader tests only. Gap: toolkit package mount adapter.
- [ ] EMU-PROBE-004 - Probe may read filenames, headers, lightweight indexes, and must reject protected/encrypted content without DRM bypass. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: unsupported/protected diagnostics and legal guardrails.
- [ ] EMU-PROBE-005 - Mount-only default blocks modification of foreign source and never imports old VN as Astra canonical source. Design: `goals.md`, `compatibility-layer.md`. Code: none for AstraEmu. Evidence: none. Gap: enforcement tests and compatibility report.

## Compat Core Runtime Contract

- [ ] EMU-CORE-001 - `ICompatRuntimeProvider` defines describe, probe, load content, step, write/read save section or capture/restore snapshot. Design: `legacy-compatibility-contract.md`, `compatibility-layer.md`. Code: none. Evidence: none. Gap: public API and ABI boundary.
- [ ] EMU-CORE-002 - Compat Core `Step` runs under `RuntimeTickInput` and does not own the main loop. Design: `legacy-compatibility-contract.md`. Code: none for AstraEmu; `RuntimeTickInput` exists. Evidence: runtime tests only. Gap: mock core step test.
- [ ] EMU-CORE-003 - RuntimeEvent sequence numbers are assigned by Runtime, not Compat Core. Design: `legacy-compatibility-contract.md`. Code: none for AstraEmu. Evidence: none. Gap: adapter and deterministic test.
- [ ] EMU-CORE-004 - Compat Core emits RuntimeEvent, PresentationCommand, and optional TextCaptureEvent DTOs. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: DTOs, mapper, sample core.
- [ ] EMU-CORE-005 - Core-specific control flow remains private and never becomes AstraVN native source syntax. Design: `script-and-presentation.md` shared VN semantics. Code: none. Evidence: none. Gap: tests that reject source-language leakage.
- [ ] EMU-CORE-006 - Renderer, text, audio, decode, filesystem, package, and native handles remain private to existing providers/services. Design: `coding-style.md`, `legacy-compatibility-contract.md`. Code: none for AstraEmu. Evidence: generic public header isolation only. Gap: compat public ABI scan once API exists.

## Save, Replay, And State

- [ ] EMU-SAVE-001 - Compat VM state is stored as an opaque save section through `ISaveSectionProvider`-style boundaries. Design: `legacy-compatibility-contract.md` section 6. Code: none; generic SaveSection DTO exists. Evidence: runtime SaveV2 tests only. Gap: AstraEmu save section provider.
- [ ] EMU-SAVE-002 - Save stores VM state, logical media state refs, enhancement profile id, translation cache refs, and no native handles. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: schema and handle-forbidden tests.
- [ ] EMU-SAVE-003 - Missing Compat Core preserves save section but leaves it non-executable with diagnostics. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: load fallback and diagnostics.
- [ ] EMU-SAVE-004 - Replay records emitted event/presentation hashes and selected provider feature hashes through existing replay reports. Design: `legacy-compatibility-contract.md`, `save-replay-production-contract.md`. Code: none for AstraEmu. Evidence: NativeVN replay only. Gap: compat replay sample.
- [ ] EMU-SAVE-005 - Save-state does not pollute native AstraVN save model. Design: `TODO.md` section 20. Code: none. Evidence: none. Gap: schema isolation and regression tests.

## Presentation, Mapper, Enhancement, Translation

- [ ] EMU-MAP-001 - LegacyApiMapper maps background/CG to `VN.Background`, character/sprite to `VN.Character`, text/ruby/message to `VN.Dialogue`, select to `VN.Choice`, BGM/SE/voice to `VN.Audio`, transition/quake/flash/movie to `VN.Timeline` or effects. Design: `compatibility-layer.md` section 6. Code: none. Evidence: none. Gap: mapper implementation and coverage report.
- [ ] EMU-MAP-002 - Mapper uses AstraVN visible semantics but not AstraVN input languages. Design: `script-and-presentation.md` section 8.1. Code: none. Evidence: none. Gap: sample tests for DSL separation.
- [ ] EMU-MAP-003 - Text shaping uses TextLayout provider and does not rely on screenshot upscaling for text. Design: `compatibility-layer.md`, `media-backend-production-contract.md`. Code: none for AstraEmu; TextLayout provider exists. Evidence: media text tests only. Gap: compat text event path.
- [ ] EMU-MAP-004 - Audio routes to logical BGM/SE/voice buses through Audio provider. Design: `compatibility-layer.md`. Code: none for AstraEmu; Audio provider exists. Evidence: media audio tests only. Gap: compat audio adapter.
- [ ] EMU-ENH-001 - Enhancement profile schema covers scaling, font replacement, layer-aware filters, HD replacements, translation provider, overlay mode. Design: `compatibility-layer.md` section 7. Code: none. Evidence: none. Gap: schema, parser, validation, hot reload.
- [ ] EMU-ENH-002 - Layer-aware filters target background, character, UI, text, final through existing FilterGraph. Design: `compatibility-layer.md`, `media-runtime.md`. Code: none for AstraEmu; `FilterTarget` and `FilterProfile` exist. Evidence: media filter tests only. Gap: toolkit binding and profile sample.
- [ ] EMU-TEXT-001 - TextCaptureEvent captures provider id, script location or VM PC, speaker, original text, ruby/control metadata, stable text hash. Design: `legacy-compatibility-contract.md`, `compatibility-layer.md`. Code: none. Evidence: none. Gap: DTO and emission path.
- [ ] EMU-TEXT-002 - Translation provider bridge receives TextCaptureEvent and returns overlay PresentationCommand output by default. Design: `compatibility-layer.md` section 8. Code: none. Evidence: none. Gap: provider slot, bridge, overlay, audit.
- [ ] EMU-TEXT-003 - Translation audit logs request, response, cache, provider errors, and permissions. Design: `compatibility-layer.md`, `ai-mcp-safety-contract.md` audit rules. Code: none. Evidence: none. Gap: operation/audit log integration.
- [ ] EMU-TEXT-004 - Embedded replacement requires explicit Compat Core capability; otherwise overlay fallback is used. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: capability negotiation and fallback tests.

## Core Cold Swap

- [ ] EMU-SWAP-001 - Cold swap pauses runtime, writes compat save section, deactivates/shuts down/unloads old module, loads/initializes/activates new module, reads save section, resumes runtime. Design: `legacy-compatibility-contract.md` section 8. Code: none. Evidence: none. Gap: manager state machine and ModuleRuntime integration.
- [ ] EMU-SWAP-002 - Failed new core reloads old module and restores old save section. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: rollback tests.
- [ ] EMU-SWAP-003 - Incompatible save section keeps runtime paused with diagnostics. Design: `compatibility-layer.md` section 9. Code: none. Evidence: none. Gap: schema compatibility checks.
- [ ] EMU-SWAP-004 - Media provider rebuild failures fall back through existing provider fallback such as headless where allowed. Design: `legacy-compatibility-contract.md`. Code: none for AstraEmu; media provider fallback reports exist. Evidence: media tests only. Gap: swap-time provider fallback integration.
- [ ] EMU-SWAP-005 - Enhancement profile, translation config, font, filter, HD overlay, and mapper rule data reload without core binary swap where possible. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: hot-reload scopes.

## Artemis Prototype

- [ ] EMU-ART-001 - Artemis installed package layout probe recognizes `*.exe`, `*.dll`, `.pfs`, `.pfs.000`, `.pfs.721`, `movie/*.dat`, fonts, readme, batch files. Design: `compatibility-layer.md` Artemis v1. Code: none. Evidence: none. Gap: probe fixture and diagnostics.
- [ ] EMU-ART-002 - Artemis unpacked layout probe recognizes `font`, `image`, `pc`, `script`, `sound`, `system`, `system.ini`. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: unpacked mock fixture.
- [ ] EMU-ART-003 - Artemis startup chain resolves `system.ini -> system/first.iet -> system/init.lua -> system/script.asb -> script/*.ast`. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: resolver and report.
- [ ] EMU-ART-004 - Artemis index covers `.iet`, `.asb`, `.ast`, `.ipt`, `.sli`, `.tbl`. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: indexer and schema.
- [ ] EMU-ART-005 - Minimal Artemis `e:*` host API surface exists. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: host API mapper and safe sandbox.
- [ ] EMU-ART-006 - High-frequency tag coverage includes `bg`, `fg`, `text`, `msg`, `vo`, `se`, `bgm`, `select`, `excall`, `wait`, `extrans`, `msgoff`, `quake`, `ruby`, `eval`, `movie`. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: tag executor and coverage report.
- [ ] EMU-ART-007 - Unsupported tag/API/asset coverage report records frequency, script location, fallback, severity. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: report schema and sample.

## Samples, Tests, Docs

- [ ] EMU-SAMPLE-001 - `Samples/AstraEmuToolkit` mock local game root exists. Design: `samples-and-test-matrix.md`. Code: none. Evidence: none. Gap: synthetic fixture.
- [ ] EMU-SAMPLE-002 - Mock content reader and mock Compat Core state run outside NativeVN authoring. Design: `samples-and-test-matrix.md`. Code: none. Evidence: none. Gap: sample module and CLI/test entry.
- [ ] EMU-SAMPLE-003 - Mock Compat Core steps, emits VN presentation, captures/restores snapshot. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: automated test.
- [ ] EMU-SAMPLE-004 - Mount-only diagnostics block writes to `foreign-artemis:/` assets. Design: `samples-and-test-matrix.md`. Code: none. Evidence: none. Gap: negative test.
- [ ] EMU-SAMPLE-005 - TextCaptureEvent reaches translation provider and returns overlay output. Design: `samples-and-test-matrix.md`. Code: none. Evidence: none. Gap: provider bridge sample.
- [ ] EMU-SAMPLE-006 - Core cold-swap rollback is tested. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: old/new mock core pair.
- [ ] EMU-DOCS-001 - AstraEmu Toolkit Guide, Enhancement Guide, Compat Core Authoring Guide exist. Design: `TODO.md` section 20. Code: none. Evidence: none. Gap: docs after implementation.

## Acceptance

- [ ] EMU-ACC-001 - At least one mock Compat Core runs and outputs VN presentation. Design: `roadmap.md` Phase 18. Code: none. Evidence: none. Gap: implement mock core and test.
- [ ] EMU-ACC-002 - Save-state captures and restores opaque VM state. Design: `legacy-compatibility-contract.md`. Code: none. Evidence: none. Gap: save section provider.
- [ ] EMU-ACC-003 - Mount-only default does not modify foreign source. Design: `compatibility-layer.md`. Code: none. Evidence: none. Gap: VFS policy tests.
- [ ] EMU-ACC-004 - AstraEmu does not require changes to Core, Runtime, Asset, Media, or NativeVN source boundaries. Design: `goals.md`, `architecture.md`. Code: none. Evidence: current absence only; no toolkit test. Gap: enforce once toolkit exists.
- [ ] EMU-ACC-005 - Artemis prototype maps high-frequency tags to AstraVN Events without sharing Artemis VM control flow with AstraVN source languages. Design: `roadmap.md`, `compatibility-layer.md`. Code: none. Evidence: none. Gap: Artemis prototype.



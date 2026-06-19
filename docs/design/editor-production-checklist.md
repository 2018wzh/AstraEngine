# Editor Production Checklist

Status: Creator workflow audit ledger / Editor not implemented  
Last audited: 2026-06-19

Legend:

- [x] Code-backed complete in the current source tree, with automated test or CLI evidence.
- [~] Partial, foundation-only, or design-contract-only.
- [ ] Planned or not implemented.

Rule: for Editor-specific features, `Code: none` means no `Engine/Editor` target or public Editor API exists yet. Reusable runtime foundations are listed as prerequisites, not as Editor completion.

## Shell, Project Entry, And Templates

- [ ] ED-SHELL-001 - `AstraEditor` executable or library target exists. Design: `editor-and-pipeline.md`, ADR 0002. Code: none; `ASTRA_BUILD_EDITOR` option exists in `CMakeLists.txt` only. Evidence: none. Gap: create `Engine/Editor` target and startup shell.
- [ ] ED-SHELL-002 - Qt-based dockable workbench shell exists. Design: ADR 0002, `editor-ui-ai-collaboration-prototype.md`. Code: none. Evidence: none. Gap: implement Qt shell, docking, command routing, panel lifecycle.
- [ ] ED-SHELL-003 - Project Browser opens existing projects and recent projects. Design: `editor-and-pipeline.md`, `editor-runtime-creator-contract.md`. Code: none. Evidence: none. Gap: project descriptor discovery, validation, UI, diagnostics.
- [ ] ED-SHELL-004 - Project Wizard creates `.astra.yaml`, `Content`, default script, scene, filter profile, sample sidecars, and module policy. Design: `editor-and-pipeline.md` Project Wizard section. Code: none. Evidence: none. Gap: wizard UI, template descriptor loader, source writes, rollback.
- [ ] ED-SHELL-005 - Template Browser previews template metadata and sample screenshots without starting runtime. Design: `architecture.md` public contracts, `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: template catalog, preview assets, validation report.
- [ ] ED-SHELL-006 - Creator-facing sample launcher opens NativeVN, PackageLaunch, RuntimeStress, and future samples. Design: `samples-and-test-matrix.md`. Code: none. Evidence: none. Gap: sample registry UI and launch integration.

## Content Browser And Asset Import

- [ ] ED-CONTENT-001 - Content Browser lists source assets, sidecars, folders, AssetIds, tags, license/review state, and generated/cooked status. Design: `content-and-assets.md`, `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: UI backed by `Astra::Asset::AssetRegistryBuilder`.
- [ ] ED-CONTENT-002 - Asset Import Wizard previews decode metadata and writes source asset plus sidecar through importer contracts. Design: `asset-package-production-contract.md`, `editor-and-pipeline.md`. Code: none for Editor; runtime importer code exists in `Engine/Runtime/Asset`. Evidence: no Editor validation. Gap: wizard UI, import transaction, undo/redo, diagnostics.
- [ ] ED-CONTENT-003 - Dependency view shows hard/soft dependencies and broken references. Design: `asset-pipeline.md`. Code: none for Editor; `AssetRegistryBuilder` reports dependencies. Evidence: runtime asset tests only. Gap: dependency graph UI, filtering, jump-to-source.
- [ ] ED-CONTENT-004 - Reference repair tool proposes stable AssetId/path fixes. Design: `editor-and-pipeline.md`, `content-and-assets.md`. Code: none. Evidence: none. Gap: repair command, preview, source patch output.
- [ ] ED-CONTENT-005 - Batch rename, move, migration, and orphan draft cleanup preserve AssetIds or create reviewable patches. Design: `content-and-assets.md`. Code: none. Evidence: none. Gap: source transaction model.
- [ ] ED-CONTENT-006 - Import wizard enforces license, foreign mount policy, AI provenance, and review state before cook. Design: `ai-mcp-safety-contract.md`, `asset-package-production-contract.md`. Code: none for Editor; release gate foundation exists. Evidence: no Editor workflow test. Gap: UI and Review Queue connection.

## Scene, Actor, Component, Prefab

- [ ] ED-SCENE-001 - Scene Tree displays ActorWorld actors, lifecycle states, preview actors, and hierarchy. Design: `actor-component-ecs-hybrid.md`, `editor-runtime-creator-contract.md`. Code: none for Editor; `ActorWorld` snapshots exist. Evidence: runtime tests only. Gap: inspector session, UI tree, selection sync.
- [ ] ED-SCENE-002 - Actor Type Palette creates actors from descriptors and templates. Design: `editor-and-pipeline.md`, `actor-component-ecs-hybrid.md`. Code: none. Evidence: none. Gap: palette registry, source patch generation, validation.
- [ ] ED-SCENE-003 - Component Palette adds/removes components through typed patches and write policy. Design: `foundation-core-platform-property.md`, `editor-and-pipeline.md`. Code: none for Editor; `ComponentDescriptor` exists. Evidence: no Editor validation. Gap: property-aware add/remove transactions.
- [ ] ED-SCENE-004 - Prefab Browser lists base prefabs, variants, nested prefab policy, overrides, diff, rollback. Design: `actor-component-ecs-hybrid.md`. Code: none for Editor; prefab DTOs exist in `Scene.hpp`. Evidence: runtime prefab test only. Gap: UI authoring and source storage.
- [ ] ED-SCENE-005 - Preview attach/detach controls allow transient scene edits during authoring. Design: `editor-runtime-creator-contract.md`. Code: none for Editor; runtime preview attach DTO/path exists. Evidence: runtime scene test only. Gap: Editor overlay and promote-to-source flow.

## Inspector And Property Editing

- [ ] ED-INSP-001 - Inspector displays Actor, Component, StateMachine, Asset, Script state, and Runtime snapshot targets through public DTOs only. Design: `editor-runtime-creator-contract.md`. Code: none. Evidence: none. Gap: `IEditorRuntimeSession::Inspect` implementation and UI.
- [ ] ED-INSP-002 - Inspector consumes PropertySystem metadata: display name, category, order, tooltip, visibility, validation. Design: `foundation-core-platform-property.md`, `editor-and-pipeline.md`. Code: none for Editor; metadata exists in `PropertySystem.hpp`. Evidence: property tests only. Gap: UI widgets and metadata binding.
- [ ] ED-INSP-003 - Inspector enforces read-only, runtime-only, editor-only, requires-review, ai-editable, and release-sensitive flags. Design: `ai-mcp-safety-contract.md`, `editor-runtime-creator-contract.md`. Code: none for Editor; write policy exists in runtime PropertySystem. Evidence: property write-policy tests only. Gap: Editor edit gate and review item generation.
- [ ] ED-INSP-004 - Property edits generate structured source patches and undo transactions. Design: `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: patch schema implementation, dirty state, undo/redo stack.
- [ ] ED-INSP-005 - Runtime preview edits apply as transient overlays and never enter packages unless promoted. Design: `editor-runtime-creator-contract.md`. Code: none. Evidence: none. Gap: `PreviewOverlay`, `PromotePreviewToSource`, Review Queue integration.

## Script, Graph, Timeline, FilterGraph Authoring

- [ ] ED-AUTH-001 - Script Editor provides diagnostics, source map navigation, breakpoint placement, run/step controls, and canonical `.astra` editing. Design: `script-and-presentation.md`, `editor-and-pipeline.md`. Code: none for Editor; parser/debug DTOs exist in `AstraScript`. Evidence: script runtime tests only. Gap: editor UI and debug command bridge.
- [ ] ED-AUTH-002 - Graph Editor writes canonical graph source and does not maintain a separate preview model. Design: `editor-and-pipeline.md`, `editor-runtime-creator-contract.md`. Code: none. Evidence: none. Gap: graph schema source storage, node editing, runtime preview.
- [ ] ED-AUTH-003 - Timeline Editor writes canonical timeline source, source maps, debug symbols, hot reload policy, camera/audio/filter events. Design: `media-backend-production-contract.md`, `script-and-presentation.md`. Code: none for Editor; runtime timeline DTOs exist. Evidence: media timeline tests only. Gap: timeline UI and source patching.
- [ ] ED-AUTH-004 - FilterGraph Editor edits layer-aware filters for background, character, UI, text, final targets. Design: `media-runtime.md`, `editor-and-pipeline.md`. Code: none for Editor; `FilterProfile` runtime exists. Evidence: media tests only. Gap: visual graph UI, validation, preview.
- [ ] ED-AUTH-005 - Hot reload compatibility report is shown before applying script/graph/timeline changes to PIE. Design: `script-and-presentation.md`, `editor-runtime-creator-contract.md`. Code: none for Editor; `ScriptHotReloadReport` exists. Evidence: script hot reload test only. Gap: UI and state-compatibility command path.

## PIE And Runtime Debugger

- [ ] ED-PIE-001 - PIE starts the same RuntimeWorld, ScriptRuntimeHost, AssetRegistry, Media pipeline, and release-gate services as packaged runtime. Design: `goals.md`, `editor-runtime-creator-contract.md`. Code: none. Evidence: none. Gap: `EditorRuntimeSession` implementation and profile management.
- [ ] ED-PIE-002 - PIE changes default to transient and save snapshots to `Saved`, not `Content`. Design: `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: runtime session overlay and save routing.
- [ ] ED-DBG-001 - Runtime Debugger supports pause, step frame, resume, breakpoints, and debug command recording. Design: `editor-runtime-creator-contract.md`. Code: none for Editor; runtime/script step DTOs exist. Evidence: no Editor debug validation. Gap: debug command API and UI.
- [ ] ED-DBG-002 - Event Log displays emitted, queued, deferred, target-aware, priority-ordered events. Design: `runtime-production-contract.md`, `editor-and-pipeline.md`. Code: none for Editor; runtime event trace exists. Evidence: runtime tests only. Gap: event log panel and filters.
- [ ] ED-DBG-003 - Debugger shows queued scheduler tasks, wait conditions, task state, and wake reasons. Design: `runtime-production-contract.md`. Code: none for Editor; scheduler snapshot exists. Evidence: runtime scheduler tests only. Gap: task inspector panel.
- [ ] ED-DBG-004 - Debugger shows ControlPolicy locks, Director phase, timeline lock, choice lock, AI permission window, and arbitration log. Design: `runtime-core.md`, `editor-runtime-creator-contract.md`. Code: none for Editor; Director DTOs exist. Evidence: runtime Director tests only. Gap: UI and debug commands.
- [ ] ED-DBG-005 - StateMachine visual debugger shows current state, transitions, guards, actions, delayed events, and timers. Design: `runtime-core.md`, `actor-component-ecs-hybrid.md`. Code: none. Evidence: none. Gap: full state machine authoring/debug metadata.

## Save, Replay, Asset, Package, Observability Panels

- [ ] ED-OBS-001 - Save/Replay Inspector shows SaveV2 section tree, schema versions, migration path, snapshot diff, replay checkpoints, mismatch localization. Design: `save-replay-production-contract.md`. Code: none for Editor; SaveV2 and replay DTOs exist. Evidence: runtime replay tests only. Gap: inspector UI.
- [ ] ED-OBS-002 - Asset Dependency Inspector shows registry, dependency graph, broken refs, package payload, DDC status, and hot reload rollback. Design: `asset-package-production-contract.md`. Code: none for Editor; asset DTOs exist. Evidence: asset tests only. Gap: panel UI and repair actions.
- [ ] ED-OBS-003 - Cook/Package panel runs validate, cook, package, release-gate, and previews package manifest/report/blocking diagnostics. Design: `editor-and-pipeline.md`, `tools-release-observability.md`. Code: none for Editor; CLI services exist. Evidence: CLI tests only. Gap: Editor panel and shared service binding.
- [ ] ED-OBS-004 - Output Log panel displays structured logs, diagnostic mirroring, component channels, and recent log ring. Design: `release-gate-observability-contract.md`. Code: none for Editor; core logging exists. Evidence: logging tests only. Gap: UI viewer.
- [ ] ED-OBS-005 - Diagnostics panel groups machine-readable diagnostics by source object, severity, fix suggestion, and release-blocking status. Design: `foundation-core-platform-property.md`. Code: none. Evidence: none. Gap: panel and quick-fix commands.
- [ ] ED-OBS-006 - Profiler/trace viewer shows runtime tick, scheduler, asset load, media decode/render/audio, script, provider lifecycle, AI intent. Design: `release-gate-observability-contract.md`. Code: none for Editor; trace DTO evidence exists. Evidence: release-gate runtime DTO tests only. Gap: interactive viewer and capture management.
- [ ] ED-OBS-007 - Crash Diagnostics viewer opens crash bundles with build info, diagnostics, recent logs, last frames, provider states, and minidump links. Design: `release-gate-observability-contract.md`. Code: none for Editor; crash bundle DTO evidence exists. Evidence: runtime release-gate DTO only. Gap: production crash bundle generation and viewer.

## Review Queue, Undo, Commands, Layout

- [ ] ED-WORK-001 - Review Queue stores patch, asset draft, localization draft, runtime intent preview, diagnostics, approver, apply/reject/rollback command. Design: `content-and-assets.md`, `ai-mcp-safety-contract.md`. Code: none for Editor; `ReviewQueueItem` asset DTO exists. Evidence: asset DTO tests only. Gap: queue model, UI, persistence.
- [ ] ED-WORK-002 - Undo/redo records source-level transactions for scripts, graph nodes, timeline tracks, property edits, and import operations. Design: `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: transaction stack and source patch engine.
- [ ] ED-WORK-003 - Dirty state tracks source edits, generated drafts, preview overlays, cooked outputs, and package outputs separately. Design: `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: document model.
- [ ] ED-WORK-004 - Command palette, context menu, asset picker, layout presets, and key bindings are implemented. Design: `editor-ui-ai-collaboration-prototype.md`. Code: none. Evidence: none. Gap: command registry and layout persistence.
- [ ] ED-WORK-005 - Editor layout preset schema supports panel id, dock area, visibility, command bindings, project/user overrides. Design: `architecture.md`, `editor-and-pipeline.md`. Code: none. Evidence: none. Gap: schema, persistence, UI.

## Editor AI And MCP

- [ ] ED-AI-001 - Editor Copilot MCP exposes diagnostics explanation, inline suggestions, patch proposals, validation/test/cook/release-gate assistance. Design: `ai-collaboration.md`, `ai-mcp-safety-contract.md`. Code: none. Evidence: none. Gap: MCP host, resources, tools, review policy.
- [ ] ED-AI-002 - Editor Content Generation MCP generates, modifies, enhances, previews, compares variants, accepts/rejects drafts. Design: `ai-mcp-safety-contract.md`. Code: none. Evidence: none. Gap: draft workspace, providers, review/import flow.
- [ ] ED-AI-003 - AI-generated drafts cannot enter AssetRegistry, Cook, or Package until accepted. Design: `ai-mcp-safety-contract.md`, `asset-package-production-contract.md`. Code: none for Editor/AI; asset release gate has review fields. Evidence: no AI workflow test. Gap: AI sidecar, generation audit, release-gate blocker sample.
- [ ] ED-AI-004 - Trusted session vs Review Queue write policy is enforced for mutating Editor MCP tools. Design: `mcp-integration.md`, `ai-mcp-safety-contract.md`. Code: none. Evidence: none. Gap: Boundary Manager and operation log.

## Editor Acceptance

- [ ] ED-ACC-001 - New creator can complete Template -> Project -> Content -> Script/Graph/Timeline -> PIE -> Cook -> Package without manual file edits. Design: `goals.md`, `samples-and-test-matrix.md` CreatorWorkflow. Code: none. Evidence: none. Gap: full Editor and CreatorWorkflow sample.
- [ ] ED-ACC-002 - Editor can pause, step, inspect, save, load, replay, and debug a running world through public runtime APIs. Design: `editor-runtime-creator-contract.md`. Code: none. Evidence: none. Gap: EditorRuntimeSession and debugger panels.
- [ ] ED-ACC-003 - Packaged runtime launches after Editor closes and has no Editor dependency. Design: `goals.md`. Code: runtime package path exists, Editor code none. Evidence: NativeVN package tests prove runtime side only. Gap: Editor integration still absent.
- [ ] ED-ACC-004 - Editor manual, PIE guide, Runtime Debugger guide, Inspector guide, and creator tutorial exist and match implementation. Design: `TODO.md` section 14. Code: none. Evidence: docs are skeleton/planned. Gap: write docs after implementation.



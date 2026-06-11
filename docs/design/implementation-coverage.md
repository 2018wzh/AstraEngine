# Implementation Coverage Matrix

状态：Target Architecture Audit Index  
定位：把 `docs/design` 的设计规格映射到可实现 artifact、验收证据和非目标，防止文档只停留在概念层。

## 1. Coverage Rule

每个核心系统必须至少有：

- design spec。
- public contract or schema。
- implementation TODO。
- validation or release gate rule。
- sample or test evidence。
- explicit non-goal or boundary。

如果某系统缺少其中任一项，不能声明为 UE-class 2D runtime complete。

## 2. System Matrix

| System | Design Spec | Public Contracts | TODO Section | Evidence |
| --- | --- | --- | --- | --- |
| Goals / scope | `goals.md` | success state, non-goals | TODO 1, 19 | release acceptance |
| Architecture | `architecture.md` | dependency matrix, public contracts | TODO all | coverage audit |
| Foundation | `foundation-core-platform-property.md` | diagnostics, config, serialization, Platform, PropertySystem | TODO 3, 4, 6 | unit, ABI scan, headless |
| Module / Plugin | `extension-and-module-system.md` | descriptor, C ABI, ServiceRegistry, ExtensionRegistry, EngineModuleSlot | TODO 5, 15 | Module ABI tests, CustomizationPlugin |
| Runtime Core | `runtime-core.md` | RuntimeWorld, RuntimeEvent, Scheduler, StateMachine, Save/Replay | TODO 8, 9 | RuntimeStress, NativeVN replay |
| Actor / Component | `actor-component-ecs-hybrid.md` | ActorId, ComponentDescriptor, Inspector metadata, prefab | TODO 7 | Actor tests, CreatorWorkflow |
| Asset Pipeline | `asset-pipeline.md`, `content-and-assets.md` | AssetId, sidecar, importer, cooker, DDC, package manifest | TODO 10 | PackageSmoke, release gate |
| Media Runtime | `media-runtime.md` | Renderer2D/TextLayout/Audio provider, FilterGraph, Timeline | TODO 11 | MediaBackend |
| Script / Presentation | `script-and-presentation.md` | ScriptRuntimeHost, Script API, DSL IR, PresentationCommand, AstraVN | TODO 12, 13 | ScriptParity, NativeVN |
| Editor / Pipeline | `editor-and-pipeline.md`, `editor-ui-ai-collaboration-prototype.md` | workflow contracts, layout preset, undo/redo, PIE | TODO 14 | CreatorWorkflow |
| AI Collaboration | `ai-collaboration.md` | Runtime AI MCP, Editor Copilot MCP, Content Generation MCP | TODO 16 | AIIntentSafety, CreatorWorkflow |
| MCP Integration | `mcp-integration.md` | Editor/Runtime MCP hosts, resources/tools/prompts | TODO 16, 17 | MCP tool tests |
| Tools / Release / Observability | `tools-release-observability.md` | CLI output, release report, trace, crash bundle | TODO 17 | release commands |
| Samples / Tests | `samples-and-test-matrix.md` | sample descriptor, test descriptor | TODO 18 | CI/local command output |
| Legacy Expansion | `compatibility-layer.md` | CompatRuntimeProvider, PackageReader, LegacyApiMapper, Save extension | TODO 20 | CompatMockExpansion |

## 3. Deliverable Matrix

Phase 0 evidence means documentation and build-baseline evidence. Phase 1 foundation evidence includes dynamically linked `Astra*` DLLs, `AstraCore`, `AstraPlatform`, `AstraModuleRuntime`, `AstraPropertySystem`, `AstraExampleFoundationPlugin`, `AstraTools`, `AstraPhaseTests`, foundation sample descriptors, CLI smoke commands, and `foundation_core_gate` release evidence. Phase 2 foundation evidence includes `AstraScene` and the Runtime module. Phase 3 foundation evidence includes `AstraAsset`, `AstraMedia`, Asset/Media tests, public header isolation, NativeVN/ArtemisVN source sidecars, AssetRegistry/dependency graph evidence, DDC metadata DTOs, local DDC artifact write/reuse/corruption recovery evidence, embedded package payload table, PackageReader random-access/chunked-read/mount evidence, package/cook/payload integrity diagnostics, mature media backend capability reports, image/font/audio decode metadata, package-payload libpng RGBA image primitive present evidence, package-payload HarfBuzz/FreeType glyph primitive present evidence, CLI Asset/Media/FilterGraph smoke hashes, and media provider release-gate foundation reports. Phase 4 playable evidence includes `AstraScript`, `AstraAstraVN`, Native DSL/Lua parity tests, Script/AstraVN public header isolation, `Samples/NativeVN` playable v1 sources, `Samples/ArtemisVN` local fixture UI/system evidence, CLI `phase4_script_vn` and `playable_vn` evidence, VN session save/restore hashes, package manifest evidence, and golden replay comparison. Runtime deliverables beyond these evidence slices remain target acceptance evidence and are incomplete until the relevant systems and samples exist.

| Deliverable | Required Evidence |
| --- | --- |
| Phase 0 manual baseline | `docs/manual` required pages and `tools/doc-check.ps1` output |
| Phase 0 build baseline | CMake configure/build and CTest discovery output |
| Phase 1 Core foundation | Core headers, `AstraCore`, diagnostics/config/stable-id/serialization tests, diagnostic code registry, release policy, release config hash, unknown-field policy |
| Phase 1 Platform foundation | `AstraPlatform`, headless service tests, opaque dynamic library handle, file-watch/pending-task/crash context tests, SDL private compile path, public header isolation scan |
| Phase 1 Module foundation | descriptor validation, dependency resolver, C ABI headers, service resolve audit, engine module slot policy validation, example plugin load/register/unload test, module release-gate binary SHA-256 evidence |
| Phase 1 Property foundation | `AstraPropertySystem`, nested struct/array/map/tagged union JSON Schema, defaults/validation/migration tests, schema version graph, write policy and release-sensitive diff/audit tests |
| Phase 1 foundation CLI | `astra --version`, `doc-check`, `validate`, `inspect`, `foundation_core_gate`, foundation-only `cook/package/run --headless-smoke` |
| Dynamic engine linking | `Astra*` runtime/tool DLLs in `build/Bin`, generated per-module export headers, plugin MODULE targets, engine/plugin binary SHA-256 evidence |
| Phase 1 sample descriptors | `Samples/NativeVN`, `Samples/RuntimeStress`, `Samples/PackageSmoke`, `Samples/ArtemisVN` descriptors |
| Phase 2 Scene foundation | `AstraScene`, ActorWorld spawn/destroy/snapshot tests, ComponentDescriptor tests, headless local ECS pack, EnTT private header isolation |
| Phase 2 Runtime foundation | Runtime module, RuntimeWorld event/state-machine/save/load/replay hash tests, ControlPolicy allow/queue/reject tests |
| Phase 3 Asset foundation | `AstraAsset`, AssetUri parse/normalize tests, VFS priority/read-only tests, sidecar validation, registry scan, dependency diagnostics, NativeVN source asset sidecars, DDC metadata entries, local DDC artifact write/reuse/corruption recovery tests, embedded package payload table, PackageReader random-access/chunked-read/mount tests, package/cook/payload integrity checks |
| Phase 3 Media foundation | `AstraMedia`, PresentationCommand/RenderGraph DTO tests, FilterProfile target validation/application, Renderer2D/TextLayout/Audio provider descriptor validation, mature backend capability report for SDL3/libpng/libjpeg-turbo/libwebp/FreeType/HarfBuzz/miniaudio, PNG/JPEG/WebP image metadata inspect API, libpng image decode smoke, image cook artifact metadata tests, media release-gate foundation reports, HeadlessRenderer2D deterministic hashes, SDL private compile-path stub |
| Phase 4 Script foundation | `AstraScript`, ScriptRuntimeHost, Native DSL parser diagnostics, Lua provider via `sol2`, shared command stream, ScriptEventBridge, ScriptSnapshot, Native/Lua parity hashes |
| Phase 4 AstraVN foundation | `AstraAstraVN`, VN event schemas, preset actors/components/state machines, VnSession, VnSessionSnapshot, NativeVN headless playable smoke, save/restore evidence |
| Runtime can launch without Editor | PackageSmoke command output and package manifest without Editor modules |
| NativeVN playable package | NativeVN `cook/package/run --headless-smoke/run --windowed-smoke/replay --compare/inspect` reports with package manifest, embedded payload table, PackageReader payload/mount smoke, local DDC artifacts, package integrity diagnostics, generated PNG/OGG fixture evidence, UI/system evidence, save/load evidence, and Script/AstraVN evidence |
| ArtemisVN local playable fixture | ArtemisVN `local_test_only` package/run/replay/inspect reports with copied Artemis PNG/OGG/font/UI/system assets, curated Aya route, system menu, backlog, config, save/load slots, and fixture report |
| Deterministic runtime | repeated replay state/event/presentation hash |
| Real media backend | Playable v1 media decode/playback evidence plus SDL/headless RGBA image and HarfBuzz/FreeType glyph primitive present evidence from package payloads in NativeVN/ArtemisVN reports; MediaBackend production visual/audio/headless hash reports remain the broader target |
| Script debug and snapshot | ScriptParity debugger and save/replay reports |
| Creator-friendly workflow | CreatorWorkflow tutorial and Editor smoke output |
| Plugin/provider customization | CustomizationPlugin build/load/release reports |
| Runtime AI safety | AIIntentSafety save/replay/audit reports |
| Release Gate correctness | blocking scenario reports |
| Legacy remains expansion | CompatMockExpansion only under expansion profile |

## 4. Non-goal Matrix

| Non-goal | Guard |
| --- | --- |
| Complex 3D/FPS/open-world parity | Goals, Roadmap and UE-class acceptance non-goals |
| UE UObject/UHT/GC parity | Foundation and PropertySystem boundaries |
| Editor as runtime dependency | Architecture dependency matrix, PackageSmoke |
| AI provider in Core | AI/MCP boundary and Core forbidden deps |
| Runtime MCP project write | MCP Integration Runtime tools policy |
| Legacy before native parity | Roadmap/TODO expansion ordering |
| Native handle in public ABI | Foundation ABI scan and Module ABI tests |
| Unreviewed AI content in package | Asset/AI release gate |

## 5. Completion Audit Procedure

To claim docs/design complete for implementation planning:

1. Enumerate all files in `docs/design`.
2. Confirm every major system in the System Matrix has a design spec and TODO reference.
3. Confirm every public contract appears in at least one design spec and glossary or README entry.
4. Confirm every acceptance scenario maps to a sample/test descriptor.
5. Confirm non-goals are present in goals, roadmap and relevant subsystem docs.
6. Run search checks for stale wording:
   - `AI\s+Workbench`
   - `真实.*后端已完成`
   - `Runtime.*依赖.*Editor`
   - `Legacy.*前置`
7. Treat missing evidence as incomplete, not as passed.

## 6. 验收

- README links every design spec.
- README links the manual root.
- Phase 0 doc-check validates required manual pages and local links.
- Phase 1 tests validate foundation runtime targets and example module lifecycle.
- TODO references every implementation-critical design spec.
- Samples/Test Matrix maps acceptance to evidence.
- Coverage Matrix has no major system without design, contract, TODO and evidence.
- Search checks do not find stale or contradictory wording.

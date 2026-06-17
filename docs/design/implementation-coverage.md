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
| Runtime Core | `runtime-core.md`, `runtime-production-contract.md`, `save-replay-production-contract.md` | RuntimeWorld, RuntimeEvent, Scheduler, StateMachine, Save/Replay | TODO 8, 9 | RuntimeStress, NativeVN replay |
| Actor / Component | `actor-component-ecs-hybrid.md` | ActorId, ComponentDescriptor, Inspector metadata, prefab | TODO 7 | Actor tests, CreatorWorkflow |
| Asset Pipeline | `asset-pipeline.md`, `content-and-assets.md`, `asset-package-production-contract.md` | AssetId, sidecar, importer, cooker, DDC, package manifest | TODO 10 | PackageSmoke, release gate |
| Media Runtime | `media-runtime.md`, `hardware-media-decode.md`, `media-backend-production-contract.md` | DecodeProvider, Renderer2D/TextLayout/Audio provider, FilterGraph, Timeline | TODO 11 | MediaBackend |
| Script / Presentation | `script-and-presentation.md` | ScriptRuntimeHost, Script API, DSL IR, PresentationCommand, AstraVN | TODO 12, 13 | ScriptParity, NativeVN |
| Editor / Pipeline | `editor-and-pipeline.md`, `editor-ui-ai-collaboration-prototype.md`, `editor-runtime-creator-contract.md` | workflow contracts, layout preset, undo/redo, PIE | TODO 14 | CreatorWorkflow |
| AI Collaboration | `ai-collaboration.md`, `ai-mcp-safety-contract.md` | Runtime AI MCP, Editor Copilot MCP, Content Generation MCP | TODO 16 | AIIntentSafety, CreatorWorkflow |
| MCP Integration | `mcp-integration.md` | Editor/Runtime MCP hosts, resources/tools/prompts | TODO 16, 17 | MCP tool tests |
| Tools / Release / Observability | `tools-release-observability.md`, `release-gate-observability-contract.md` | CLI output, release report, trace, crash bundle | TODO 17 | release commands |
| Samples / Tests | `samples-and-test-matrix.md` | sample descriptor, test descriptor | TODO 18 | CI/local command output |
| Legacy Expansion | `compatibility-layer.md`, `legacy-compatibility-contract.md` | CompatRuntimeProvider, PackageReader, LegacyApiMapper, Save extension | TODO 20 | CompatMockExpansion |

## 2.1 Production Contract Matrix

| Contract Document | Primary Interfaces / DTOs | Sample Evidence |
| --- | --- | --- |
| `runtime-production-contract.md` | `RuntimeTickInput`, `RuntimeFrameResult`, `SchedulerTaskDescriptor`, `WaitCondition`, `DirectorArbitrationRequest` | RuntimeStress |
| `save-replay-production-contract.md` | `SaveSectionDescriptor`, `SaveContainerV2`, `ReplayStream`, `ReplayCheckpoint`, `ReplayMismatchReport` | RuntimeStress, ScriptParity |
| `asset-package-production-contract.md` | `ImportRequest`, `CookRequest`, `CookArtifactDescriptor`, `DdcKey`, `PackagePayloadRef`, `PackageMountPolicy` | PackageSmoke |
| `hardware-media-decode.md` | `DecodeProviderDescriptor`, `DecodeCapability`, `DecodeRequest`, `DecodedCpuBuffer`, future decoded PCM/video frame DTOs, `MediaSurfaceToken` | MediaBackend |
| `media-backend-production-contract.md` | `IRenderer2DProvider`, `ITextLayoutProvider`, `IAudioProvider`, Timeline and FilterGraph provider contracts | MediaBackend |
| `provider-contracts.md` | `ProviderDescriptor`, `ProviderCapabilitySet`, `ProviderSelectionPolicy`, `ProviderShutdownContract` | CustomizationPlugin |
| `editor-runtime-creator-contract.md` | `EditorRuntimeSession`, `InspectRequest`, `DebugCommand`, `PreviewOverlay`, `SourcePatchProposal` | CreatorWorkflow |
| `ai-mcp-safety-contract.md` | `AIIntent`, `IntentValidationResult`, `CommittedAIOutput`, `GenerationAuditRecord`, `ReviewQueueItem` | AIIntentSafety |
| `legacy-compatibility-contract.md` | `ICompatRuntimeProvider`, `ILegacyPackageReader`, `LegacyVmSnapshot`, `SaveExtensionStateProvider` | CompatMockExpansion |
| `release-gate-observability-contract.md` | `ReleaseReport`, `BlockingPolicy`, `TraceEvent`, `CrashBundle` | PackageSmoke, CustomizationPlugin, RuntimeStress |

## 3. Deliverable Matrix

Phase 0 evidence means documentation and build-baseline evidence. Phase 1 foundation evidence includes dynamically linked `Astra*` DLLs, `AstraCore`, `AstraPlatform`, `AstraModuleRuntime`, `AstraPropertySystem`, `AstraExampleFoundationPlugin`, `AstraTools`, `AstraPhaseTests`, foundation sample descriptors, CLI smoke commands, and `foundation_core_gate` release evidence. Phase 2 foundation evidence includes `AstraScene` and the Runtime module. Phase 3 foundation evidence includes `AstraAsset`, `AstraMedia`, Asset/Media tests, public header isolation, NativeVN/TsuiNoSora source sidecars, AssetRegistry/dependency graph evidence, mature media backend capability reports, image/font/audio decode metadata, package-payload libpng RGBA image primitive present evidence, package-payload HarfBuzz/FreeType glyph primitive present evidence, CLI Asset/Media/FilterGraph smoke hashes, and media provider release-gate foundation reports. Phase 4 playable evidence includes `AstraScript`, `AstraAstraVN`, Native DSL/Lua parity tests, Script/AstraVN public header isolation, `Samples/NativeVN` playable sources, `Samples/TsuiNoSora` local fixture UI/system evidence, CLI `phase4_script_vn` and `playable_vn` evidence, and VN session save/restore hashes. Phase 5/6 evidence includes package manifest hash/provider feature hash in save/replay reports, replay mismatch localization, production Asset importer/cooker contracts, local DDC write/reuse/corruption recovery evidence, binary `.astrapkg` writer/reader, zstd payload table, PackageReader random-access/chunked-read/mount evidence, Asset Release Gate blockers, package/cook/payload integrity diagnostics, package-only launch evidence, and golden replay comparison. Phase 7 evidence includes production media provider descriptors, image/audio/video decode slots, decoded CPU texture buffers, audio metadata decode, glyph-run/atlas capture, logical audio mixer state, video decode extension-point diagnostics, timeline camera/audio/filter state, FilterGraph execution hashes, and `phase7_media_backend` CLI evidence. Editor/AI/Legacy, PCM sample decode, video frame decode, GPU shader execution, and per-driver visual/audio diff deliverables remain target acceptance evidence until those systems and samples exist.

| Deliverable | Required Evidence |
| --- | --- |
| Phase 0 manual baseline | `docs/manual` required pages and `astra doc-check` output |
| Phase 0 build baseline | CMake configure/build and CTest discovery output |
| Phase 1 Core foundation | Core headers, `AstraCore`, diagnostics/config/stable-id/serialization tests, diagnostic code registry, release policy, release config hash, unknown-field policy |
| Phase 1 Platform foundation | `AstraPlatform`, headless service tests, opaque dynamic library handle, file-watch/pending-task/crash context tests, SDL private compile path, public header isolation scan |
| Phase 1 Module foundation | descriptor validation, dependency resolver, C ABI headers, service resolve audit, engine module slot policy validation, example plugin load/register/unload test, module release-gate binary SHA-256 evidence |
| Phase 1 Property foundation | `AstraPropertySystem`, nested struct/array/map/tagged union JSON Schema, defaults/validation/migration tests, schema version graph, write policy and release-sensitive diff/audit tests |
| Phase 1 foundation CLI | `astra --version`, `doc-check`, `validate`, `inspect`, `foundation_core_gate`, foundation-only `cook/package/run --headless-smoke` |
| Dynamic engine linking | `Astra*` runtime/tool DLLs in `build/Bin`, generated per-module export headers, plugin MODULE targets, engine/plugin binary SHA-256 evidence |
| Phase 1 sample descriptors | `Samples/NativeVN`, `Samples/RuntimeStress`, `Samples/PackageSmoke`, `Samples/TsuiNoSora` descriptors |
| Phase 2 Scene foundation | `AstraScene`, ActorWorld spawn/destroy/snapshot tests, ComponentDescriptor tests, headless local ECS pack, EnTT private header isolation |
| Phase 2 Runtime foundation | Runtime module, RuntimeWorld event/state-machine/save/load/replay hash tests, ControlPolicy allow/queue/reject tests |
| Phase 6 Asset pipeline | `AstraAsset`, AssetUri parse/normalize tests, VFS priority/read-only tests, sidecar validation, registry scan, dependency diagnostics, NativeVN source asset sidecars, `ImportRequest`/`ImporterDescriptor` tests, built-in importer/cooker descriptors, `CookAssetRegistry`, local DDC write/reuse/corruption recovery tests, binary `.astrapkg` writer/reader, zstd payload table, PackageReader random-access/chunked-read/mount tests, Asset Release Gate blockers, hot reload rollback DTO tests, package/cook/payload integrity checks |
| Phase 3 Media foundation | `AstraMedia`, PresentationCommand/RenderGraph DTO tests, FilterProfile target validation/application, Renderer2D/TextLayout/Audio provider descriptor validation, mature backend capability report for SDL3/libpng/libjpeg-turbo/libwebp/FreeType/HarfBuzz/miniaudio, PNG/JPEG/WebP image metadata inspect API, libpng image decode smoke, image cook artifact metadata tests, media release-gate foundation reports, HeadlessRenderer2D deterministic hashes, SDL private compile-path stub |
| Phase 4 Script foundation | `AstraScript`, ScriptRuntimeHost, Native DSL parser diagnostics, Lua provider via `sol2`, shared command stream, ScriptEventBridge, ScriptSnapshot, Native/Lua parity hashes |
| Phase 4 AstraVN foundation | `AstraAstraVN`, VN event schemas, preset actors/components/state machines, VnSession, VnSessionSnapshot, NativeVN headless playable smoke, save/restore evidence |
| Phase 5 Runtime core evidence | Runtime module target-aware deterministic event ordering, subscription lifetime, serializable scheduler tasks, `astra.runtime.save_container.v2` with optional zstd JSON sections, replay mismatch localization, RuntimeStress 1000 Actor save/load/replay hash stability |
| Runtime can launch without Editor | PackageSmoke command output and package manifest without Editor modules |
| NativeVN playable package | NativeVN `cook/package/run --headless-smoke/run --windowed-smoke/replay --compare/inspect` reports with binary `.astrapkg` package manifest, zstd payload table, PackageReader payload/mount smoke, local DDC artifacts, Asset Release Gate evidence, package manifest hash/provider feature hash save-replay evidence, package integrity diagnostics, generated PNG/OGG fixture evidence, UI/system evidence, save/load evidence, and Script/AstraVN evidence |
| TsuiNoSora local playable fixture | TsuiNoSora `local_test_only` package/run/replay/inspect reports with copied Artemis PNG/OGG/font/UI/system assets, curated Aya route, system menu, backlog, config, save/load slots, and fixture report |
| Deterministic runtime | repeated replay state/event/presentation hash |
| Real media backend | Phase 7 provider/decode/timeline/filter evidence plus SDL/headless RGBA image and HarfBuzz/FreeType glyph primitive present evidence from package payloads in NativeVN/TsuiNoSora reports; per-driver visual/audio diff remains the broader hardening target |
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

# Samples And Test Matrix 设计

状态：NativeVN runtime evidence scaffold / Target Architecture  
定位：用样例项目和测试矩阵证明 AstraEngine 的 UE-class 2D runtime 完备度。样例不是演示摆设，而是 release gate、文档、CLI、Editor 和 Runtime 的共同验收载体。

Current implementation note：当前已建立 `Samples/NativeVN`、`Samples/RuntimeStress`、`Samples/PackageSmoke` 和 `Samples/TsuiNoSora` 的 foundation/evidence descriptors。`PackageSmoke` 可通过 `astra validate/package/run --headless-smoke` 证明 Phase 1 headless platform 和 example module lifecycle；`NativeVN` 可通过 validate/cook/package/run/replay/inspect 证明 source asset sidecars、generated PNG/OGG/font fixture media、AssetRegistry/dependency graph、local DDC artifact evidence、embedded package payloads、PackageReader mount/read smoke、Script/AstraVN execution、playable UI/system/save/load evidence、package-payload SDL/headless RGBA image and HarfBuzz/FreeType glyph primitive present evidence 和 golden replay comparison。`TsuiNoSora` 是 local-test-only playable fixture，用复制的 Artemis PNG/OGG/font/UI/system 资源验证真实资源 registry/cook/package/windowed/headless/run evidence，以及来自 package payload 的真实 PNG texture primitive 与 glyph primitive 提交，不代表 Legacy VM、完整 `.ast` converter 或 redistributable sample。Editor workflow、final release gate、production binary streaming 和 full Artemis compatibility 仍是后续阶段。

## 1. 目标

Samples/Test Matrix 必须覆盖：

- creator workflow：模板、导入/生成资产、Script/Graph/Timeline、PIE、打包。
- runtime workflow：launch、tick、save、load、replay、debug、profile。
- customization workflow：plugin、provider replacement、Editor panel、MCP tool。
- AI workflow：Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP。
- release workflow：validate、cook、package、run、replay、inspect、doc-check、ctest。

每个 sample 必须有：

- project descriptor。
- expected output。
- golden replay or validation report。
- release profile。
- troubleshooting。
- manual tutorial。
- CI command。

## 2. Sample Projects

目录：

```text
Samples/
├─ NativeVN
├─ RuntimeStress
├─ PackageSmoke
├─ TsuiNoSora
├─ ScriptParity
├─ MediaBackend
├─ AIIntentSafety
├─ CreatorWorkflow
├─ CustomizationPlugin
└─ AstraEmuToolkit
```

Sample descriptor：

```yaml
id: astra.sample.native_vn
display_name: Native VN
purpose: UE-class native runtime acceptance
project_path: Samples/NativeVN/NativeVN.astra.yaml
release_profiles: [development, deterministic]
requires_editor: false
requires_network: false
golden:
  replay: build/Saved/Replays/NativeVNGolden.replay
  package_hash: Saved/Reports/NativeVN.package.hash
commands:
  validate: astra validate Samples/NativeVN --strict --json
  cook: astra cook Samples/NativeVN --config Release --json
  package: astra package Samples/NativeVN --profile deterministic --json
  run: astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --json
  replay: astra replay build/Saved/Replays/NativeVNGolden.replay --compare --json
acceptance:
  - no_editor_launch
  - deterministic_replay
  - release_gate_pass
```

## 3. NativeVN

Purpose：

- Complete native AstraVN vertical slice。

Must include：

- background。
- character sprite/emotion。
- dialogue。
- choice。
- voice/music/SFX。
- timeline。
- filter profile。
- camera。
- save/load/replay。
- package launch without Editor。

Acceptance：

- `validate -> cook -> package -> run -> replay -> inspect` pass。
- packaged runtime shows/records real image/text/audio/filter output。
- save after choice can reload to same route state。
- golden replay state/event/presentation hash match。

## 4. RuntimeStress

Purpose：

- Prove Runtime Core scale and determinism。

Must include：

- 1000+ Actor。
- multiple StateMachine components per actor。
- queued/deferred/scheduled events。
- scheduler waits for event/time/asset。
- repeated save/load checkpoints。
- long-run soak profile。
- `runtime-production-contract.md` tick/scheduler/Director coverage。
- `save-replay-production-contract.md` checkpoint and mismatch report coverage。

Acceptance：

- no handle reuse errors。
- event queue drains within policy。
- memory/resource lifetime stable。
- state hash stable across repeated runs。
- trace captures frame/runtime bottlenecks。

## 5. PackageSmoke

Purpose：

- Prove packaged runtime has no Editor dependency。

Must include：

- minimal cooked package。
- runtime-safe module set。
- package manifest。
- CLI run in headless mode。
- `asset-package-production-contract.md` PackageReader/package mount policy coverage。
- `release-gate-observability-contract.md` release report and blocking diagnostic coverage。

Acceptance：

- package starts from `Saved/Packages` only。
- no source Content read。
- no Editor, authoring MCP, debug-only module dependency。
- package hash matches manifest。

## 6. ScriptParity

Purpose：

- Prove Native DSL、Lua、Graph/Timeline paths share Runtime semantics。

Must include：

- equivalent dialogue/choice flow in Native DSL and Lua。
- Graph source for at least one branch。
- Timeline with camera/audio event。
- debugger symbols and source maps。
- `save-replay-production-contract.md` script decision, source map and replay mismatch coverage。

Acceptance：

- authored-equivalent paths produce equivalent RuntimeEvent and PresentationCommand hashes。
- breakpoints work in DSL and Graph。
- script snapshot/restore survives wait state。

## 7. MediaBackend

Purpose：

- Prove real media backend and headless verification。

Must include：

- Decode Provider selection for `astra.image_decode`、`astra.audio_decode` and optional `astra.video_decode`。
- texture decode/upload。
- sprite batching/layer order。
- text shaping/font fallback。
- voice/music/SFX routing。
- executable FilterGraph targets。
- frame capture metadata。
- `hardware-media-decode.md` capability/fallback/zero-copy diagnostics coverage。
- `media-backend-production-contract.md` Renderer2D/TextLayout/Audio/FilterGraph execution coverage。

Acceptance：

- headless render/text/audio/filter hashes match expected。
- selected Renderer2D/TextLayout/Audio providers pass release gate。
- selected Decode providers pass release gate or emit allowed fallback diagnostics。
- missing glyph/asset/audio cases produce diagnostics。

## 8. AIIntentSafety

Purpose：

- Prove Runtime AI MCP can generate controlled content safely and deterministically。

Must include：

- player feedback input。
- runtime context inspect。
- AIIntent request/validate/commit。
- fallback select。
- committed output saved and replayed without provider。
- `ai-mcp-safety-contract.md` Review Queue, generation audit and provider-free replay coverage。

Acceptance：

- Runtime MCP cannot project write。
- deterministic profile blocks runtime provider。
- hybrid profile allows provider only with audit/fallback policy。
- replay uses committed output only。

## 9. CreatorWorkflow

Purpose：

- Prove UE-level creator experience。

Must include：

- Project Wizard from template。
- Content Browser import of character/background/audio/font/filter。
- Editor Content Generation draft and Review Queue accept/reject。
- Script/Graph/Timeline editing。
- Inspector property edit with undo/redo。
- PIE debug and package。
- `editor-runtime-creator-contract.md` EditorRuntimeSession, InspectRequest, DebugCommand and SourcePatchProposal coverage。

Acceptance：

- new creator can complete tutorial without manual file edits。
- all source mutations produce undo transaction or review item。
- generated draft cannot cook before review accepted。
- package launches without Editor。

## 10. CustomizationPlugin

Purpose：

- Prove plugin author workflow and provider replacement。

Must include：

- Plugin Wizard output。
- sample `IEditorPanelProvider`。
- sample `IAssetImporter`。
- sample Renderer2D/TextLayout/Audio provider or provider stub。
- sample `IMcpToolProvider` read-only tool。
- plugin descriptor and release checklist。
- `provider-contracts.md` ProviderDescriptor, capability negotiation, selection policy and shutdown contract coverage。

Acceptance：

- plugin builds, validates, loads, registers, unloads。
- provider selected through EngineModuleSlot policy。
- invalid permission is blocked by Release Gate。
- public ABI forbidden type scan passes。

## 11. AstraEmuToolkit

Purpose：

- Prove AstraEmu is a standalone toolkit and does not participate in NativeVN creation workflow。

Must include：

- mock local game root。
- mock content reader。
- mock Compat Core state。
- LegacyApiMapper output to VN events。
- enhancement profile。
- save-state。
- TextCaptureEvent and translation Provider bridge。
- core cold-swap rollback。
- `legacy-compatibility-contract.md` AstraEmuManager, CompatRuntimeProvider, ILegacyContentReader, LegacyVmSnapshot and TextCapture boundary coverage。

Optional local fixture：

- Anonymous Artemis 2025 VN case study may be used on developer machines only。
- Fixture path is supplied by local environment or test configuration and is never committed。
- Default CI uses synthetic/mock data; real commercial assets are not copied into the repository。

Artemis-specific scenarios：

- probe installed package layout：`exe`、`dll`、`.pfs`、`.pfs.000`、`.pfs.721`、`movie/*.dat`、font/readme/batch files。
- probe unpacked layout：`font`、`image`、`pc`、`script`、`sound`、`system`、`system.ini`。
- decode or index `.iet`、`.asb`、`.ast` script entry and label/block metadata。
- produce host API coverage report for Artemis `e:*` calls。
- produce tag coverage report for high-frequency story tags and unsupported tags。
- verify LegacyApiMapper emits AstraVN Events and PresentationCommand data。
- verify mount-only policy blocks writes to `foreign-artemis:/` assets。
- verify save-state stores Artemis VM cursor、variables and call stack as opaque compat state。
- verify TextCaptureEvent can reach a translation Provider and return overlay output。

Acceptance：

- AstraEmu toolkit sample runs outside NativeVN project authoring。
- native samples build/package without AstraEmu module。
- foreign assets remain mount-only。
- save-state loads without changing native save schema。

## 12. Test Matrix

```text
unit
  Core / Property / AssetId / EventBus / StateMachine / Module ABI
integration
  ActorWorld / ScriptRuntimeHost / Asset Cook / Media Headless / Save Replay
headless
  NativeVN / PackageSmoke / AIIntentSafety
smoke
  Module load/unload / CLI commands / Plugin Wizard generated project
stress
  RuntimeStress / large content / long-run soak / hot reload rollback
compat
  AstraEmuToolkit / mount-only / mapper fallback / TextCapture
release-gate
  missing dependency / unreviewed AI / invalid license / invalid plugin permission / runtime AI deterministic block
doc
  required manual pages / links / snippets / public API coverage
```

Each test must declare：

```yaml
test_id: astra.test.native_vn.package_launch
category: headless
sample: astra.sample.native_vn
requires: [package]
command: astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --json
evidence:
  - diagnostics_report
  - trace_capture
  - state_hash
```

## 13. Completion Evidence

UE-class 2D runtime acceptance requires current evidence, not intent：

- sample project source exists。
- package can be produced。
- package launches without Editor。
- golden replay matches。
- release gate report passes。
- trace/crash/diagnostic reports are generated。
- docs/manual tutorial exists and doc-check passes。
- CI or local command output proves tests ran。

## 14. 验收

- Every sample has descriptor、commands、expected output、tutorial and troubleshooting。
- Every TODO acceptance category maps to at least one sample or test category。
- `NativeVN` is the final UE-class acceptance sample。
- `CustomizationPlugin` proves tool/plugin authors can extend without Runtime edits。
- `CreatorWorkflow` proves authoring ergonomics and review/undo/package flow。
- `AstraEmuToolkit` proves legacy compatibility remains a standalone toolkit after native parity。

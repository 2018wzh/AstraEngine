# Samples And Test Matrix 设计

状态：Target Architecture  
定位：用样例项目和测试矩阵证明 AstraEngine 的 UE-class 2D runtime 完备度。样例不是演示摆设，而是 release gate、文档、CLI、Editor 和 Runtime 的共同验收载体。

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
├─ ScriptParity
├─ MediaBackend
├─ AIIntentSafety
├─ CreatorWorkflow
├─ CustomizationPlugin
└─ CompatMockExpansion
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
  replay: Saved/Replays/NativeVNGolden.replay
  package_hash: Saved/Reports/NativeVN.package.hash
commands:
  validate: astra validate Samples/NativeVN --strict --json
  cook: astra cook Samples/NativeVN --config Release --json
  package: astra package Samples/NativeVN --profile deterministic --json
  run: astra run Saved/Packages/NativeVN.astrapkg --headless-smoke --json
  replay: astra replay Saved/Replays/NativeVNGolden.replay --compare --json
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

Acceptance：

- authored-equivalent paths produce equivalent RuntimeEvent and PresentationCommand hashes。
- breakpoints work in DSL and Graph。
- script snapshot/restore survives wait state。

## 7. MediaBackend

Purpose：

- Prove real media backend and headless verification。

Must include：

- texture decode/upload。
- sprite batching/layer order。
- text shaping/font fallback。
- voice/music/SFX routing。
- executable FilterGraph targets。
- frame capture metadata。

Acceptance：

- headless render/text/audio/filter hashes match expected。
- selected Renderer2D/TextLayout/Audio providers pass release gate。
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

Acceptance：

- plugin builds, validates, loads, registers, unloads。
- provider selected through EngineModuleSlot policy。
- invalid permission is blocked by Release Gate。
- public ABI forbidden type scan passes。

## 11. CompatMockExpansion

Purpose：

- Prove legacy expansion track does not pollute native runtime。

Must include：

- mock foreign project root。
- mock package reader。
- mock legacy VM state。
- LegacyApiMapper output to VN events。
- modernization profile。
- save extension state。

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
- verify mount-only policy blocks copying `foreign-artemis:/` assets。
- verify save extension state stores Artemis VM cursor、variables、call stack and package mounts as opaque compat state。

Acceptance：

- compat sample runs only when expansion profile enabled。
- native samples build/package without compat module。
- foreign assets remain mount-only。
- save extension state loads/replays without changing native save schema。

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
  CompatMockExpansion / mount-only / mapper fallback
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
command: astra run Saved/Packages/NativeVN.astrapkg --headless-smoke --json
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
- `CompatMockExpansion` proves legacy remains post-parity expansion track。

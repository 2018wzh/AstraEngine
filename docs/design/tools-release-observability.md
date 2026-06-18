# Tools / Release Gate / Observability 设计

状态：NativeVN runtime evidence CLI plus Phase 6 Asset Pipeline / Target Architecture  
定位：Astra 的 CLI、验证、Cook/Package、发布门禁、profiling、trace、crash/error report 和测试矩阵。

Current implementation note：当前 `astra` 使用 CLI11 实现 `--version`、`doc-check`、`validate`、`import`、`cook`、`package`、`run --headless-smoke`、`replay --compare` 和 `inspect`。报告使用 JSON；`astra validate . --strict --json` 输出 `foundation_core_gate` artifact，包含 registered diagnostic-code gate、release config hash、unknown-field policy evidence、Property write/schema evidence、Module release-gate report、engine DLL SHA-256 evidence 和插件 binary SHA-256。Core logging 已使用 private `spdlog` backend 实现 `astra.log.event.v1` JSONL、async rotating file sink、console sink、memory test capture、recent-log ring、diagnostic mirroring，以及 CLI `--log-dir/--log-file/--log-level/--log-async/--log-sync`。`Samples/NativeVN` 已覆盖 source asset sidecars、AssetRegistry/dependency graph evidence、local DDC artifact write/reuse/corruption recovery、binary `.astrapkg` zstd payload table、PackageReader random-access/chunked-read/mount evidence、Asset Release Gate evidence、Phase 7 media provider/decode/timeline/filter evidence、package manifest hash/provider feature hash save-replay evidence、package/cook/payload integrity diagnostics、headless package launch smoke 和 golden replay comparison。trace export/crash bundle 和 deeper per-driver replay UI/audio diff 仍是后续目标。

## 1. 目标

Developer tools 必须让 runtime 可以像 UE 去掉 Editor 后一样独立验证和发布：

- CLI、Editor、MCP 调用同一 validation/cook/package/replay/inspect service。
- Release Gate 用 machine-readable diagnostics 阻止不可发布内容。
- Observability 覆盖 runtime frame、asset load、script tick、media execution、AI intent、plugin lifecycle。
- Crash/error report 可定位 build、package、frame、module、diagnostic 和 recent trace。
- CI 能跑 unit、integration、headless、smoke、stress、compat、release-gate。

## 2. Astra CLI

命令：

```text
astra validate <project-or-package>
astra import <project> <source-file> --asset-id <native:/...> [--type <type>] [--preset <preset>]
astra cook <project> --config <Debug|Release|Profile>
astra package <project> --profile <profile> [--deterministic]
astra run <package> [--headless-smoke]
astra replay <replay> --compare
astra inspect <asset|package|save|replay|plugin>
astra doc-check
astra plugin validate <plugin>
```

Global flags：

```text
--json
--diagnostics-out <path>
--trace-out <path>
--profile <profile>
--platform <platform>
--no-editor
--strict
```

Command output contract：

```yaml
command: astra cook
status: failed
duration_ms: 1234
build_info:
  engine_version: 0.1.0
  git_commit: abc123
diagnostics:
  - code: ASTRA_RELEASE_ASSET_004
    severity: blocking
artifacts:
  report: Saved/Reports/cook-report.json
```

## 3. Validation Service

Validation passes：

- project config schema。
- plugin descriptor schema。
- EngineModuleSlot selection。
- property schema and migration。
- asset sidecar and dependency graph。
- script/graph/timeline compile。
- media provider support。
- AI review/audit policy。
- MCP/tool/provider release eligibility。
- compat expansion policy when enabled。

Validation result：

```yaml
schema: astra.validation.report.v1
target: Projects/NativeVN
profile: Release
summary:
  info: 12
  warning: 3
  error: 0
  blocking: 1
  fatal: 0
diagnostics: []
```

## 4. Cook / Package Service

Cook stages：

```text
Validate
  -> Scan AssetRegistry
  -> Build dependency graph
  -> Build cook plan
  -> Run cook processors
  -> Write DDC and cook manifest
  -> Run release gate preflight
```

Package stages：

```text
Resolve runtime modules
  -> Verify plugin ABI and permissions
  -> Copy runtime-safe binaries
  -> Copy cooked artifacts
  -> Write package manifest
  -> Hash package
  -> Run package launch smoke
  -> Write package report
```

Package report：

```yaml
schema: astra.package.report.v1
package: Saved/Packages/NativeVN.astrapkg
package_hash: sha256:...
release_profile: deterministic
assets:
  count: 120
  total_bytes: 340000000
modules:
  included: [astra.runtime, astra.vn]
  excluded: [astra.editor, astra.mcp.editor]
release_gate:
  status: passed
```

## 5. Release Gate

Release Gate profiles：

- `development`：warnings allowed, source hot reload allowed, editor MCP allowed。
- `deterministic`：runtime AI/MCP disabled, no unreviewed AI draft, deterministic package required。
- `hybrid_ai`：runtime AI allowed only with explicit provider, fallback, audit and committed-output policy。
- `astra_emu_toolkit`：standalone AstraEmu modules and local foreign mounts allowed with mount-only policy。

Blocking categories：

- invalid schema or migration。
- missing hard dependency。
- unreviewed AI draft。
- invalid license。
- forbidden foreign copy。
- invalid plugin permission。
- ABI incompatibility。
- provider not packaged eligible。
- runtime AI enabled without release profile。
- Editor dependency in packaged runtime。
- package hash mismatch。

Gate decision schema：

```yaml
schema: astra.release_gate.report.v1
profile: deterministic
status: blocked
blocking_diagnostics:
  - ASTRA_RELEASE_AI_001
non_blocking_diagnostics:
  - ASTRA_ASSET_SOFTREF_002
policy_hash: sha256:...
```

## 6. Observability

Trace event：

```yaml
time_ns: 120003000
frame: 120
category: runtime.event
name: dispatch
duration_ns: 22000
fields:
  event_type: astra.vn.dialogue.say_requested
  actor: actor:/systems/dialogue
```

Required channels：

- tools lifecycle and diagnostics。
- platform lifecycle, dynamic library, and window presentation。
- module lifecycle and service resolve audit。
- asset cook/package。
- runtime frame。
- event dispatch。
- scheduler task。
- state machine transition。
- script command。
- asset load。
- media render/text/audio/filter。
- AI intent validation/commit。
- module load/unload。
- save/load/replay。

Implemented logging channels follow these names today; future trace capture should reuse the same channel taxonomy where possible.

Profiler output：

- frame time。
- tick time。
- script time。
- asset load time。
- render extraction time。
- media backend time。
- audio mix time。
- memory/resource lifetime summary。

## 7. Crash / Error Report

Crash bundle：

```text
crash/
├─ build-info.json
├─ diagnostics.json
├─ last-log.txt
├─ trace-last-frames.json
├─ runtime-summary.json
├─ package-manifest-summary.json
├─ module-list.json
└─ minidump.dmp (platform optional)
```

Runtime summary：

- frame index。
- scene id。
- actor count。
- event queue length。
- scheduler task count。
- script runtime states。
- active package hash。
- last save/replay checkpoint。

Rules：

- Crash report never includes secrets。
- AI prompt/context is redacted unless debug policy explicitly includes hashed metadata。
- Foreign file paths can be redacted by privacy policy。

## 8. Build Info

`AstraBuildInfo`：

```yaml
engine_version: 0.1.0
git_commit: abc123
build_config: Release
target_platform: win64
feature_flags:
  runtime_ai: false
  editor: false
abi_versions:
  module: astra.module.abi.v1
  save: astra.save.v1
  package: astra.package.v1
```

Build info is embedded in：

- executable。
- package manifest。
- diagnostics report。
- crash bundle。
- trace capture。

## 9. Tests And CI Matrix

Test categories：

- `unit`：Core、Property、AssetId、EventBus、StateMachine。
- `integration`：ActorWorld、ScriptRuntimeHost、Asset cook、Media headless。
- `headless`：NativeVN gameplay path、save/replay、package launch。
- `smoke`：module load/unload、CLI commands。
- `stress`：1000+ Actor、large content、long-run soak。
- `compat`：mock legacy runtime fixture and mount-only policy。
- `release-gate`：blocking scenarios。

Required release commands：

```powershell
astra validate Samples/NativeVN --strict --json
astra cook Samples/NativeVN --config Release --json
astra package Samples/NativeVN --profile deterministic --json
astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --json
astra replay build/Saved/Replays/NativeVNGolden.replay --compare --json
astra inspect build/Saved/Packages/NativeVN.astrapkg --json
astra doc-check --json
ctest --test-dir build -C Release --output-on-failure
```

## 10. MCP And Editor Integration

Editor invokes tools through services, not shell-only behavior：

- Validate panel。
- Cook/Package panel。
- Release Gate report viewer。
- Trace viewer。
- Crash report viewer。
- Replay mismatch viewer。

MCP tool rules：

- read-only session can run inspect/validate only。
- review session can create patch/draft/review item。
- trusted session can apply source patch。
- no MCP session writes Cooked/DDC/package manifest directly。

## 11. 验收

- CLI can validate、cook、package、run、replay、inspect NativeVN without Editor。
- Release Gate blocks unreviewed AI draft、invalid plugin permission、missing dependency、runtime AI in deterministic profile and Editor dependency in package。
- Trace captures runtime event、script、asset、media、AI intent and module lifecycle channels。
- Crash bundle contains build info、diagnostics、last logs、trace summary and package/module summary。
- CI matrix can run release commands and classify failures by diagnostic code。

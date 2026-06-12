# Release Gate / Observability Contract

状态：Production contract draft / partially implemented as foundation reports  
定位：定义 release report、diagnostics policy、trace/profiling 和 crash bundle 的生产契约。本文补足 `tools-release-observability.md` 的实现级输出结构。

## 1. 目标

- Release Gate 用 machine-readable diagnostics 阻止不可发布内容。
- CLI、Editor、MCP 和 CI 消费同一 validation/cook/package/release report schema。
- Trace/profiling 覆盖 runtime tick、scheduler、asset load、media backend、script、provider lifecycle 和 AI intent。
- Crash bundle 包含 build info、diagnostics、recent logs、last frame summary 和 package/project hash。

非目标：

- Log 不替代 diagnostics。
- Profiling UI 不是 Runtime dependency。

## 2. Release Report

Release report:

```yaml
schema: astra.release.report.v1
target: Samples/NativeVN
profile: deterministic
passed: false
reports:
  validation: Saved/Reports/validate.json
  cook: Saved/Reports/cook.json
  package: Saved/Reports/package.json
diagnostics: []
provider_policy_hash: "..."
package_manifest_hash: "..."
```

Blocking policy:

```yaml
schema: astra.release.blocking_policy.v1
profile: deterministic
blocking_severities: [blocking, fatal]
deny_runtime_ai_provider: true
require_provider_binary_hash: true
require_save_migration: true
allow_media_fallback: false
```

Rules:

- Release Gate evaluates diagnostics severity and profile policy.
- Non-blocking warnings remain in report and may require explicit waiver in release profile.
- Provider dependency closure and EngineModuleSlot selection must be included.

## 3. Validation Domains

Required production domains:

- Core diagnostics and registered codes.
- Platform backend packaged eligibility.
- Module ABI, permissions, binary hash.
- Provider descriptors and feature negotiation.
- Asset sidecars, dependencies, license/review state.
- Cook artifacts and DDC/package integrity.
- Media decode/render/audio/text/filter capability.
- Runtime save/replay/migration compatibility.
- Script compile/source map/sandbox.
- Editor dependency exclusion for package.
- AI review/audit/committed output policy.

## 4. Trace And Profiling

Trace event:

```yaml
schema: astra.trace.event.v1
time_ns: 123456
frame_index: 120
channel: runtime.scheduler
name: task_wake
duration_ns: 3000
objects:
  - kind: task
    id: task:/opening/typewriter
fields: {}
```

Channels:

- `runtime.tick`
- `runtime.event`
- `runtime.scheduler`
- `scene.lifecycle`
- `asset.load`
- `asset.cook`
- `media.decode`
- `media.render`
- `media.audio`
- `script.execute`
- `provider.lifecycle`
- `ai.intent`
- `module.lifecycle`

Rules:

- Trace captures identifiers and timings, not native handles.
- Deterministic replay can compare trace checkpoints when enabled.

## 5. Crash Bundle

Crash bundle:

```yaml
schema: astra.crash.bundle.v1
build_info: {}
package_or_project_hash: "..."
frame_index: 120
thread_id: "..."
last_runtime_hashes: {}
recent_diagnostics: []
recent_logs: []
last_events: []
provider_states: []
minidump_path: ""
```

Rules:

- Crash bundle must be safe to generate after provider failure.
- Provider state summaries are DTOs; no native handles.
- Fatal diagnostics include crash bundle path when available.

## 6. Acceptance

Required samples/tests:

- `PackageSmoke`: release report blocks missing package dependency.
- `CustomizationPlugin`: invalid provider permission blocks release.
- `MediaBackend`: unsupported codec/render feature blocks or falls back by profile.
- `RuntimeStress`: trace captures frame timing and scheduler timing.
- `AIIntentSafety`: deterministic profile blocks unauthorized runtime AI provider.

CLI acceptance commands remain:

```powershell
astra validate Samples/NativeVN --strict --json
astra cook Samples/NativeVN --config Release --json
astra package Samples/NativeVN --profile deterministic --json
astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --json
astra replay build/Saved/Replays/NativeVNGolden.replay --compare --json
astra inspect build/Saved/Packages/NativeVN.astrapkg --json
```


# Release Gate Checks Blueprint

Release Gate check 必须是 machine-readable、可复现、可从 Editor/CLI/CI/MCP 统一调用。每个 check 都有 id、domain、输入、阻断条件、evidence 和验证命令。

## Check Record

```rust
pub struct ReleaseCheckRecord {
    pub id: CheckId,
    pub domain: ReleaseDomain,
    pub status: CheckStatus,
    pub source_ref: Option<SourceRef>,
    pub diagnostic: Option<DiagnosticCode>,
    pub evidence: EvidenceMap,
}
```

## Required Matrix

| Domain | Check ID | Input | Blocking Condition | Evidence |
| --- | --- | --- | --- | --- |
| runtime | `runtime.replay.determinism` | scenario report | hash mismatch | state/event/presentation hash |
| target | `target.manifest` | package target manifest | missing target, not exactly one packaged Game, selected target absent | target id, kind, profile |
| plugin | `plugin.fingerprint` | plugin descriptor | version or feature mismatch | descriptor hash |
| plugin | `plugin.extension_registry` | extension registration report | conflict, missing phase, invalid extension point or packaged trim error | extension id, phase, plugin id |
| plugin | `plugin.dependency_graph` | plugin enablement report | missing required dependency or unresolved version conflict | dependency edge, selected provider |
| package | `package.integrity` | package container | invalid section/hash/bounds | section table hash |
| package | `package.cooked_project` | package `compiled.project` section | release profile package lacks cook/project artifact, wrong schema or mismatched package metadata | section id, schema, target, profile |
| media | `media.decode.capability` | platform report | required codec missing | provider id, codec list |
| vn | `vn.full_playthrough` | VN scenario | route/system story failure | route id, command id |
| vn | `vn.commercial_baseline` | VN release profile | dialogue/system flow baseline missing | command coverage, route coverage |
| vn | `vn.system_ui_profile` | system story report | save/load/config/backlog/gallery/replay/chart/voice/localization missing | system story id, state hash |
| vn | `vn.advanced_presentation` | opt-in scenario report | advanced profile missing or nondeterministic | timeline id, provider capability |
| editor | `editor.source_roundtrip` | editor report | source map identity failure | source_ref, command id |
| editor | `editor.plugin_manager` | plugin manager report | enablement/dependency/diagnostic jump failure | plugin id, extension id |
| ai_mcp | `ai.provider_profile` | provider descriptor, project binding | fingerprint, secret handle, network egress, runtime eligibility or model fingerprint missing | provider id, profile id, model fingerprint |
| ai_mcp | `ai.runtime_provider_startup` | release profile, platform capability | Live AI provider required by profile is unavailable at startup | provider profile, platform id, diagnostic |
| ai_mcp | `ai.provider_free_replay` | save/replay | provider required during replay | committed output hash |
| ai_mcp | `ai.runtime_memory_policy` | memory ledger, policy | canon write exceeds policy, ledger missing or vector index treated as authority | namespace, entry hash, policy id |
| ai_mcp | `ai.debug_trace_redaction` | package/report/debug profile | release artifact contains plaintext prompt, player text, commercial payload or secret | trace id, redaction status |
| ai_mcp | `ai.player_consent` | runtime profile, save memory | cloud provider reads player memory without first-run consent | consent id, provider profile |
| ai_mcp | `mcp.context_permission` | MCP audit | read/search/tool call exceeds session scope or Context Pack is not redacted | session id, tool id, source ref |
| ai_mcp | `mcp.command_allowlist` | MCP command report | undeclared command or arbitrary shell execution | command id, template id |
| platform | `platform.eligibility` | capability report | profile requirement missing | platform id, capability id |
| platform | `platform.capability_report` | capability report | missing SDK, missing required smoke, blocked required smoke or invalid schema | platform id, SDK status, smoke id, diagnostic |
| emu | `emu.artemis_full_flow` | local case report | trace/snapshot/redaction failure | trace hash, redaction status |
| emu | `emu.legacy_runtime_provider` | family plugin report | family bypasses RuntimeWorld or missing provider session binding | family id, provider id, session id |
| emu | `emu.auto_probe` | auto probe report | selected family is not reproducible or override reason missing | selected family, priority list, override reason |
| emu | `emu.trusted_luau_policy` | trusted script report | denied capability mutates runtime or script isolation missing | script id, denied capability, isolation status |
| emu | `emu.text_redaction` | text pipeline report | report contains full commercial text without local opt-in | text hash, source ref, dump policy |
| emu | `emu.filter_preset` | filter preset report | preset bypasses FilterGraph validation or leaks native handle | preset id, target layer, validation status |

`desktop-release` 和 `web-release` 默认要求 `compiled.project` 与 `platform.capability_report`。Release package 必须来自 `astra cook`/project 输入，`PackageBuildRequest::minimal` 这类 fixture package 只能用于 dev/headless 测试，不能冒充发布输入。缺 platform report 时是 blocking；headless/dev profile 可降为 warning。Desktop release 缺 `windowed_smoke`、`renderer.wgpu_surface`、`decode.wmf.audio`、`decode.wmf.video_first_frame`、`audio.wasapi` 或 `save.known_folder_rw` 时必须 blocked。Web release 使用同一 check；真实浏览器缺 `browser_smoke`、`renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw` 或 `package.web_source_read` 时，check 必须是 `blocked`，不能降级成 fallback pass。

## Report Schema

```yaml
schema: astra.release_report.v1
package_id: com.example.nativevn
profile: desktop-release
status: blocked
checks:
  - id: runtime.replay.determinism
    domain: runtime
    status: pass
    evidence:
      state_hash: hash128:...
  - id: emu.artemis_full_flow
    domain: emu
    status: blocked
    diagnostic: ASTRA_EMU_REDACTION_FAILED
    source_ref: null
  - id: target.manifest
    domain: target
    status: pass
    evidence:
      target_count: 1
  - id: vn.advanced_presentation
    domain: vn
    status: pass
    evidence:
      timeline_hash: hash128:...
  - id: ai.runtime_memory_policy
    domain: ai_mcp
    status: pass
    evidence:
      memory_namespace: cast.hero
  - id: plugin.extension_registry
    domain: plugin
    status: pass
    evidence:
      extension_count: 42
```

## Commands

```bash
astra package validate target/nativevn.astrapkg --profile desktop-release --report target/release_report.yaml
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/scenario_report.yaml
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/artemis_report.yaml
cargo test -p astra-release release_report
```

Expected report: 每个 domain 至少一个 check；blocked check 必须有 diagnostic 和 evidence；report 不包含商业 payload、provider secret、native handle 或私有绝对路径。

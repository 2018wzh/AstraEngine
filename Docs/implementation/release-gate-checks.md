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
| runtime | `runtime_provider.binding` | target manifest, provider descriptor | missing gameplay runtime provider, fingerprint mismatch, profile not supported or provider selected by load order | target id, runtime provider id, profile, descriptor hash |
| package | `package.integrity` | package container | invalid section/hash/bounds | section table hash |
| package | `package.cooked_project` | package `compiled.project` section | release profile package lacks cook/project artifact, wrong schema or mismatched package metadata | section id, schema, target, profile |
| package | `vfs.package_mount` | package section table, VFS mount descriptor | section ref missing, hash mismatch, bounds invalid, codec unsupported or schema mismatch | mount id, section id, hash, offset, size |
| package | `vfs.local_authorized_mount` | authorized mount descriptor, host capability | alias missing, relative key invalid, permission missing, root leaks into report or source hash mismatch | alias, key count, redaction status |
| package | `vfs.legacy_pack_mount` | legacy pack reader report | reader identity missing, entry table hash missing, duplicate key, unsupported compression, offset/size out of bounds or media kind unknown | reader id, pack alias, entry count, hash |
| package | `vfs.overlay_mount` | overlay policy, base mount report | overlay lacks allowlist, priority conflict, base mount missing, source hash mismatch or payload/path leak | overlay id, base mount id, priority, key pattern count |
| media | `media.decode.capability` | platform report | required codec missing | provider id, codec list |
| vn | `vn.compiled_story` | package `vn.compiled_story` section | classic/modern profile 缺 section、解码失败、schema 错误或无 story/state | story hash, story count, state count, route node count |
| vn | `vn.profile_manifest` | package `vn.profile_manifest` section | classic/modern profile 缺 section、未声明 validation profile 或 target 不匹配 | target, profile, profile count |
| vn | `vn.full_playthrough` | VN scenario | route/system story failure | route id, command id |
| vn | `vn.commercial_baseline` | VN release profile | dialogue/system flow baseline missing | command coverage, route coverage |
| vn | `vn.compiled_story` | `classic` / `modern` VN profile | missing story, variable or command manifest evidence | story hash, story count, command manifest count, route node count |
| vn | `vn.policy_bundle` | `classic` / `modern` VN profile | missing standard policy bundle, capability, lock hash, source cache or matching source hash | bundle count, source cache section, diagnostic count |
| vn | `vn.extension_bindings` | `classic` / `modern` VN profile | missing or duplicate VN provider binding | binding count, diagnostic count |
| vn | `vn.standard_commands` | `classic` / `modern` VN profile | missing standard command manifest, unknown command usage, missing required attr or movie fallback | command count, checked usage count, diagnostic count |
| vn | `vn.presentation_provider` | `classic` / `modern` VN profile | missing presentation provider manifest, filter fallback policy or await capability | filter count, fallback count, wait capability count |
| vn | `vn.commercial_baseline` | `classic` / `modern` VN profile | missing commercial baseline manifest or required feature coverage | story hash, required count, feature count, diagnostic count |
| vn | `vn.system_ui_profile` | `classic` / `modern` VN profile | required system page missing or missing policy binding | page count, required count, missing count |
| vn | `vn.system_ui_profile` | package `vn.system_story_manifest` and `vn.system_ui_profile_manifest` sections | save migration missing, gallery/replay unlock source missing, localization coverage missing, or save/load/config/backlog/gallery/replay/chart/voice/localization missing | page count, required count, unlock source count, localization locale count, save migrator, diagnostic count |
| vn | `vn.advanced_presentation` | opt-in `vn.advanced_presentation_manifest` 和 scenario report | advanced profile 缺多层 stage、camera、video layer、timeline join/cancel、fallback、voice sync 或 effect budget evidence | story hash, timeline id count, evidence count |
| vn | `runtime_provider.native_vn` | target manifest, VN provider descriptor, package sections | missing `NativeVnRuntimeProvider`, VN package sections not bound through provider, release checks not declared or replay hash mismatch | provider id, package section count, release check count, replay hash |
| player | `player.full_playable` | Windows/Web live player automation report | missing input transcript, missing platform host evidence, unchanged or blank visual region, silent required audio meter, direct runtime command path, DOM synthetic click or dump-dom route runner | platform, input event source, focus state, region hash before/after, audio meter summary, route check count |
| vn | `tsuinosora.reference_evidence` | package `tsuinosora.reference_evidence` section | missing section, schema mismatch, non-pass status, missing reference hash, fixed `Title.png`/`Game.png` hash or dimension mismatch, path leak or payload-like field leak | reference count, section id, diagnostic |
| vn | `tsuinosora.asset_analysis` | package `tsuinosora.asset_analysis` section | empty asset evidence, quarantine asset, schema mismatch, non-pass status, path leak or payload-like field leak | asset count, quarantine count, diagnostic |
| vn | `tsuinosora.conversion_manifest` | package `tsuinosora.conversion_manifest` section | route coverage missing, empty converted resource evidence, missing source/native/classification/hash/byte size resource field, schema mismatch, non-pass status, path leak or payload-like field leak | route count, uncovered count, resource count, invalid field, diagnostic |
| vn | `tsuinosora.mount_policy` | package `tsuinosora.mount_policy` section | selected target mismatch, empty alias list, schema mismatch, non-pass status, path leak or payload-like field leak | target id, alias count, diagnostic |
| vn | `tsuinosora.modern_profile_report` | `modern` profile package section | missing report, non-reversible feature, missing fallback hash, core-state mutation, schema mismatch, path leak or payload-like field leak | feature count, diagnostic |
| vn | `tsuinosora.manual_signoff` | formal release profile package section | missing signoff, missing required manual check, wrong check id field, failed signoff item, blocker present, schema mismatch, non-pass status, path leak or payload-like field leak | check count, required check count, failed count, missing required count, blocker count |
| editor | `editor.source_roundtrip` | editor report | source map identity failure | source_ref, command id |
| editor | `editor.plugin_manager` | plugin manager report | enablement/dependency/diagnostic jump failure | plugin id, extension id |
| ai_mcp | `ai.provider_profile` | provider descriptor, project binding | fingerprint, secret handle, network egress, runtime eligibility or model fingerprint missing | provider id, profile id, model fingerprint |
| ai_mcp | `ai.model_bundle` | ModelBundle manifest, package section table | manifest missing, payload routed through `package_sections`, section ref/hash/codec/migration missing or license/provenance missing | bundle id, section id, hash, license status |
| ai_mcp | `ai.onnx_runtime_pack` | runtime vendor cache, package/VFS mount | reduced runtime not locked, release downloads runtime, VFS mount unresolved or custom op sidecar lacks hash/license/platform declaration | runtime fingerprint, VFS mount id, sidecar id |
| ai_mcp | `ai.model_bundle_vfs_mount` | ModelBundle manifest, Asset VFS report | model, tokenizer, runtime dependency or custom op sidecar reads loose shipping path, VFS locator unresolved or redaction missing | bundle id, mount id, section id, locator hash |
| ai_mcp | `ai.onnx_execution_provider` | platform capability, target runtime smoke | required primary EP missing, operator coverage incomplete, CPU fallback observed or target run evidence missing | platform id, EP, model fingerprint, operator coverage |
| ai_mcp | `ai.runtime_provider_startup` | release profile, platform capability | Live AI provider required by profile is unavailable at startup | provider profile, platform id, diagnostic |
| ai_mcp | `ai.provider_free_replay` | save/replay | provider required during replay | committed output hash |
| ai_mcp | `ai.generated_artifact_save` | save section, committed output | generated chunk not written to save extra section, artifact manifest missing mapping or hash/migration/encryption incomplete | artifact section id, chunk hash, validator status |
| ai_mcp | `ai.runtime_memory_policy` | memory ledger, policy | canon write exceeds policy, ledger missing or vector index treated as authority | namespace, entry hash, policy id |
| ai_mcp | `ai.debug_trace_redaction` | package/report/debug profile | release artifact contains plaintext prompt, player text, commercial payload or secret | trace id, redaction status |
| ai_mcp | `ai.player_consent` | runtime profile, save memory | cloud provider reads player memory without first-run consent | consent id, provider profile |
| ai_mcp | `mcp.context_permission` | MCP audit | read/search/tool call exceeds session scope or Context Pack is not redacted | session id, tool id, source ref |
| ai_mcp | `ai.context_pack_redaction` | Context Pack report, VFS resolve report | Context Pack contains local root, provider secret, payload body or unbounded source text | context pack id, source count, redaction status |
| ai_mcp | `mcp.command_allowlist` | MCP command report | undeclared command or arbitrary shell execution | command id, template id |
| platform | `platform.eligibility` | capability report | profile requirement missing | platform id, capability id |
| platform | `platform.capability_report` | capability report | missing SDK, missing required smoke, blocked required smoke or invalid schema | platform id, SDK status, smoke id, diagnostic |
| emu | `emu.artemis_full_flow` | local case report | trace/snapshot/redaction failure | trace hash, redaction status |
| emu | `emu.game_runtime_provider` | target manifest, `AstraEmuRuntimeProvider` descriptor, local case report | missing provider binding, provider does not create RuntimeWorld, save/replay hash missing or family bypasses provider | provider id, target id, session id, replay hash |
| emu | `emu.legacy_runtime_provider` | family plugin report | family bypasses RuntimeWorld or missing provider session binding | family id, provider id, session id |
| emu | `emu.vm_state_machine_trace` | family scheduler/context trace | context ordering unstable, await boundary missing, basic block not bounded, snapshot hash mismatch or fault isolation missing | family id, context count, trace hash, snapshot hash |
| emu | `emu.legacy_pack_vfs` | legacy pack VFS report | reader identity missing, pack entry out of bounds, hash mismatch, overlay not allowed or local root/payload leaked | family id, pack alias, entry count, redaction status |
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
  - id: ai.onnx_execution_provider
    domain: ai_mcp
    status: blocked
    diagnostic: ASTRA_AI_ONNX_CPU_FALLBACK
    evidence:
      model_bundle: com.example.model.local_director
      platform: windows
      required_ep: DirectML
      observed_ep: CPU
  - id: plugin.extension_registry
    domain: plugin
    status: pass
    evidence:
      extension_count: 42
```

## Commands

```bash
astra package validate target/nativevn.astrapkg --profile desktop-release --report target/release_report.yaml
astra package bundle target/nativevn.astrapkg --profile classic --target nativevn-game --platform windows --out target/bundle/windows --format json
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/scenario_report.yaml
astra test run scenarios/advanced_presentation.yaml --package target/advancedvn.astrapkg --target advanced-vn-game --profile advanced-vn --headless --report target/advanced_report.yaml
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/artemis_report.yaml
cargo test -p astra-release release_report
```

Expected report: 每个 domain 至少一个 check；blocked check 必须有 diagnostic 和 evidence；report 不包含商业 payload、provider secret、native handle 或私有绝对路径。

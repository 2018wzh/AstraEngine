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
| plugin | `plugin.fingerprint` | plugin descriptor | version or feature mismatch | descriptor hash |
| package | `package.integrity` | package container | invalid section/hash/bounds | section table hash |
| media | `media.decode.capability` | platform report | required codec missing | provider id, codec list |
| vn | `vn.full_playthrough` | VN scenario | route/system story failure | route id, command id |
| editor | `editor.source_roundtrip` | editor report | source map identity failure | source_ref, command id |
| ai_mcp | `ai.provider_free_replay` | save/replay | provider required during replay | committed output hash |
| platform | `platform.eligibility` | capability report | profile requirement missing | platform id, capability id |
| emu | `emu.artemis_full_flow` | local case report | trace/snapshot/redaction failure | trace hash, redaction status |

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
```

## Commands

```bash
astra package validate target/nativevn.astrapkg --profile desktop-release --report target/release_report.yaml
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/scenario_report.yaml
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/artemis_report.yaml
cargo test -p astra-release release_report
```

Expected report: 每个 domain 至少一个 check；blocked check 必须有 diagnostic 和 evidence；report 不包含商业 payload、provider secret、native handle 或私有绝对路径。

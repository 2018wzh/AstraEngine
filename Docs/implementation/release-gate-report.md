# Release Gate Report

Release Gate is the only authority for package eligibility. Editor, CLI, CI and MCP call the same validators and read the same report schema.

## Report Shape

```yaml
schema: astra.release_report.v1
package_id: com.example.nativevn
profile: desktop-release
platform: windows
status: blocked
summary:
  passed: 12
  warnings: 1
  blocked: 1
evidence:
  package_hash: sha256:...
  schema_registry_hash: sha256:...
  policy_lock_hash: sha256:...
checks:
  - id: runtime.replay.determinism
    domain: runtime
    status: pass
    source_ref: null
  - id: luau.policy.snapshot
    domain: luau
    status: blocked
    source_ref: policy/cinematic.luau:40:12
    diagnostic: ASTRA_LUAU_UNSERIALIZABLE_VALUE
```

## Blocking Domains

- runtime determinism
- schema migration
- package integrity
- plugin fingerprint
- permission policy
- Luau sandbox and snapshot
- policy lock/vendor cache
- media decode capability
- save/load/replay
- full headless scenario
- platform eligibility
- AI provider-free replay
- EMU Artemis full-flow, redaction and trace

Domain/check matrix 见 [release-gate-checks.md](release-gate-checks.md)。

## Evidence Policy

Allowed evidence: hash, count, schema id, command id, source span, diagnostic code, timing, coverage percentage, redacted path class.

Forbidden evidence: full commercial text, image, audio, video, private absolute path, provider secret, native handle, decrypted payload.

## Commands

```bash
astra package validate target/nativevn.astrapkg --profile desktop-release --report target/release_report.yaml
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/scenario_report.yaml
astra report explain target/release_report.yaml
```

Expected: report schema validates, blocked checks include source span and diagnostic code.

## Tests

```bash
cargo test -p astra-release release_report
cargo test -p astra-release ai_mcp_gate
cargo test -p astra-release emu_gate
```

Expected: missing audit, provider replay, migration gap, Luau snapshot error and Artemis redaction failure all block release.

# Release Gate Contract

Release Gate 是发布前唯一权威检查。Editor、CLI、MCP 和 CI 调用同一套 validator，输出 machine-readable report。

## Report Shape

```yaml
schema: astra.release_report.v1
package_id: com.example.nativevn
profile: desktop-release
status: blocked
checks:
  - id: runtime.replay.determinism
    status: pass
  - id: plugin.fingerprint
    status: pass
  - id: plugin.extension_registry
    status: pass
  - id: media.decode.platform_fallback
    status: warning
  - id: vn.system_ui_profile
    status: pass
  - id: emu.engine_native_family
    status: pass
  - id: ai.provider_free_replay
    status: pass
  - id: scenario.full_playthrough
    status: fail
    diagnostic: scenario stopped before ending.good
```

## Blocking Domains

Runtime determinism、schema migration、package integrity、plugin fingerprint、plugin extension registry、permission policy、AI replay、Luau sandbox、media decode、VN commercial baseline、system UI profile、advanced presentation opt-in、AstraEMU engine-native family、save/load、headless scenario、platform eligibility 和 manual signoff 都可以阻止发布。

## Verification Commands

```bash
astra package validate target/nativevn.astrapkg --profile desktop-release
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless
astra report explain target/release_report.yaml
```

完整 check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。每个 check 必须声明 id、domain、input、blocking condition、evidence、source_ref 和期望输出。

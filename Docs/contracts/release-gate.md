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
  - id: media.decode.platform_fallback
    status: warning
  - id: ai.provider_free_replay
    status: pass
  - id: scenario.full_playthrough
    status: fail
    diagnostic: scenario stopped before ending.good
```

## Blocking Domains

Runtime determinism、schema migration、package integrity、plugin fingerprint、permission policy、AI replay、Lua sandbox、media decode、save/load、headless scenario、platform eligibility 和 manual signoff 都可以阻止发布。

## Verification Commands

```bash
astra package validate target/nativevn.astrapkg --profile desktop-release
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless
astra report explain target/release_report.yaml
```

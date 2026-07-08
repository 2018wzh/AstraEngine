# Release Gate Contract

Release Gate 是发布前唯一权威检查。Editor、CLI、MCP 和 CI 调用同一套 validator，输出 machine-readable report。

## Report Shape

```yaml
schema: astra.release_report.v1
package_id: com.example.nativevn
profile: desktop-release
status: blocked
checks:
  - id: target.manifest
    status: pass
  - id: runtime.replay.determinism
    status: pass
  - id: plugin.fingerprint
    status: pass
  - id: plugin.extension_registry
    status: pass
  - id: package.cooked_project
    status: pass
  - id: media.decode.platform_fallback
    status: warning
  - id: vn.system_ui_profile
    status: pass
  - id: emu.legacy_runtime_provider
    status: pass
  - id: ai.model_bundle
    status: pass
  - id: ai.onnx_execution_provider
    status: blocked
    diagnostic: ASTRA_AI_ONNX_CPU_FALLBACK
  - id: ai.provider_free_replay
    status: pass
  - id: scenario.full_playthrough
    status: blocked
    diagnostic: scenario stopped before ending.good
```

## Blocking Domains

Runtime determinism、schema migration、package integrity、cook/project artifact、Target manifest、plugin fingerprint、plugin extension registry、permission policy、AI replay、ONNX ModelBundle、ONNX Runtime pack/VFS、execution provider evidence、Luau sandbox、media decode、VN commercial baseline、system UI profile、advanced presentation opt-in、AstraEMU legacy runtime provider、save/load、headless scenario、platform eligibility 和 manual signoff 都可以阻止发布。

`desktop-release` 和 `web-release` 的 package 必须由 `astra cook` 产出 `compiled.project` section。Fixture package 或缺少 cook/project metadata 的包只能用于 dev/headless 验证，不能通过 release profile。

AstraEMU 还要检查 auto probe、Trusted Luau policy、text redaction 和 FilterGraph preset。翻译 overlay 是非权威 UI 状态，不改变 replay hash；它的 release gate 只检查 provider 绑定、redaction 和禁用策略。

## Verification Commands

```bash
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --target nativevn-game --headless
astra report explain target/release_report.yaml
```

完整 check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。每个 check 必须声明 id、domain、input、blocking condition、evidence、source_ref 和期望输出。

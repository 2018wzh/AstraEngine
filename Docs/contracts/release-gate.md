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
  - id: player.full_playable
    status: blocked
    diagnostic: player input transcript missing
  - id: emu.legacy_runtime_provider
    status: pass
  - id: ai.model_bundle
    status: pass
  - id: ai.onnx_execution_provider
    status: blocked
    diagnostic: ASTRA_AI_ONNX_CPU_FALLBACK
  - id: ai.provider_free_replay
    status: pass
  - id: rpg.agent_provider_free_replay
    status: pass
  - id: rpg.trpg.transcript_redaction
    status: blocked
    diagnostic: ASTRA_RPG_TRANSCRIPT_PAYLOAD_LEAK
  - id: scenario.full_playthrough
    status: blocked
    diagnostic: scenario stopped before ending.good
```

## Blocking Domains

Runtime determinism、schema migration、package integrity、cook/project artifact、Target manifest、plugin fingerprint、plugin extension registry、permission policy、AI replay、ONNX ModelBundle、ONNX Runtime pack/VFS、execution provider evidence、Luau sandbox、media decode、VN commercial baseline、system UI profile、advanced presentation opt-in、live player automation、AstraEMU legacy runtime provider、AstraRPG provider binding、RPG policy bundle、intent validator、committed agent output、TRPG dice/seat/transcript、CP2020 local-private adapter redaction、save/load、headless scenario、platform eligibility 和 manual signoff 都可以阻止发布。

`desktop-release` 和 `web-release` 的 package 必须由 `astra cook` 产出 `compiled.project` section。Fixture package 或缺少 cook/project metadata 的包只能用于 dev/headless 验证，不能通过 release profile。

AstraEMU 还要检查 auto probe、Trusted Luau policy、text redaction 和 FilterGraph preset。翻译 overlay 是非权威 UI 状态，不改变 replay hash；它的 release gate 只检查 provider 绑定、redaction 和禁用策略。

Stage 3 `player.full_playable` 只接受 Windows/Web 平台原生输入自动化证据。Windows 必须有真实 player window focus、Win32 `SendInput` mouse/keyboard、player event loop receipt、window/renderer region hash 变化、AudioGraph meter 与 WASAPI host evidence。Web 必须有真实 browser page、CDP session、`Input.dispatchMouseEvent`、`Input.dispatchKeyEvent`、canvas/screenshot region hash 变化、WebAudio meter 和 route evidence。缺 `player.input_transcript`、缺视觉区域变化、缺音频 meter、缺 host evidence，或发现 `VnPlayerCommand`、`--route-scenario` 自推进、`--dump-dom` route runner、DOM `element.click()`、直接 JS callback、API 可用性 smoke 冒充输入时，`player.full_playable` 必须 blocked。`input.browser`、`input.gamepad`、`input.touch` 只能作为 capability check，不能作为 playable evidence。

## Verification Commands

```bash
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --target nativevn-game --headless
astra report explain target/release_report.yaml
```

完整 check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。每个 check 必须声明 id、domain、input、blocking condition、evidence、source_ref 和期望输出。

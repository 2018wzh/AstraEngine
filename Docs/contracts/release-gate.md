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

Runtime determinism、schema migration、package integrity、cook/project artifact、Target manifest、plugin fingerprint、plugin extension registry、permission policy、AI replay、ONNX ModelBundle、ONNX Runtime pack/VFS、execution provider evidence、Luau sandbox、package font authority、media decode、VN commercial baseline、system UI profile、advanced presentation opt-in、live player automation、AstraEMU legacy runtime provider、AstraRPG provider binding、RPG policy bundle、intent validator、committed agent output、TRPG dice/seat/transcript、CP2020 local-private adapter redaction、save/load、headless scenario、platform eligibility 和 manual signoff 都可以阻止发布。

`performance.profile_budget` 只接受 [Performance Contract](performance.md) 的 `astra.performance_budget.v1` 与 `astra.performance_report.v1`。报告必须为 pass，metric 集合、sample count、unit、threshold、budget hash 和 source/package/build/profile/session identity 必须全部一致。debug test、静态 benchmark、report 文件存在、过短 run 或 blocked report 都不能升级成 release performance evidence。当前 Windows native media 已能产出 measured report，但 Release Validator 与 Player 的同 run consumer 尚在实施，因此本项不能标记完成。

`media.manifest` 通过 `font_manifest_required` 和 `font_manifest_section` 显式声明字体要求。声明 required 后，Release Gate 必须从同一 package 的 `astra.font_manifest.v1` 和 `asset.vfs_manifest` 解析字体，校验 target/profile、provider binding、package backend、section range、font hash、face 与 coverage；任何 loose file、system font 或未声明 fallback 都不能补足缺失字体。未要求字体的 profile 必须显式保持 `font_manifest_required: false`，其通过结果只说明该 profile 不消费文本字体，不证明字体能力完成。

`desktop-release` 和 `web-release` 的 package 必须由 `astra cook` 产出 `compiled.project` section。Fixture package 或缺少 cook/project metadata 的包只能用于 dev/headless 验证，不能通过 release profile。

AstraEMU 还要检查 auto probe、Trusted Luau policy、text redaction 和 FilterGraph preset。翻译 overlay 是非权威 UI 状态，不改变 replay hash；它的 release gate 只检查 provider 绑定、redaction 和禁用策略。

Stage 3 `player.full_playable` 只接受 Windows/Web 平台原生输入自动化证据。Windows 必须有真实 player window focus、Win32 `SendInput` mouse/keyboard、player event loop receipt、window/renderer region hash 变化、AudioGraph meter 与 WASAPI host evidence。Web 必须有真实 browser page、CDP session、`Input.dispatchMouseEvent`、`Input.dispatchKeyEvent`、canvas/screenshot region hash 变化、WebAudio meter 和 route evidence。缺 `player.input_transcript`、缺视觉区域变化、缺音频 meter、缺 host evidence，或发现 `VnPlayerCommand`、`--route-scenario` 自推进、`--dump-dom` route runner、DOM `element.click()`、直接 JS callback、API 可用性 smoke 冒充输入时，`player.full_playable` 必须 blocked。`input.browser`、`input.gamepad`、`input.touch` 只能作为 capability check，不能作为 playable evidence。

Migration 11 完成后，真实产品平台验收还要先通过 `headless.preflight`。它要求同一 build fingerprint、cooked package hash、input sequence hash、scenario、target 和 content identity 的 `astra.headless_run_report.v1`、`astra.headless_review.v1` 与 `astra.headless_preflight_link.v1`。Headless blocked、缺 required artifact/model review 或 identity mismatch 时，不得启动正式平台验收。该 check 仍只提供 E2 前置证据，不能让 `player.full_playable`、Windows/Web host conformance 或 E3 自动通过。

当前 `platform.headless_release_boundary` 已执行 fail-closed 隔离：package section 出现 `astra.headless_*` schema、cooked `platform.profiles` 出现 Headless launch profile，或 shipping release target 引用 `headless` platform、`astra-platform-headless`/`astra-headless` role 时必须 blocking。该检查只证明发布图隔离，不证明 Headless host 已实现。

## Verification Commands

```bash
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --target nativevn-game --headless
astra report explain target/release_report.yaml
```

上述 `--headless` 是当前入口。planned Migration 11 使用独立 `astra-headless`；实现前不能把 planned preflight 命令写成已通过证据。

完整 check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。每个 check 必须声明 id、domain、input、blocking condition、evidence、source_ref 和期望输出。

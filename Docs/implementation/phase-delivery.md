# Phase Delivery

全系列 v1 按 Stage 1-6 推进。Stage 7/8 是 v1 之后的 AstraRPG 扩展计划，不作为当前 v1 release gate。每个 Stage 都要产出可运行闭环、固定命令和 machine-readable report；不能只交付内部库。

## Stage 1：EngineCore

**闭环：** native smoke package 在 headless Runtime 中启动、推进、save/load、replay，并通过 plugin descriptor gate。

**Test IDs:** `T-S1-BOOT-01`、`T-S1-SAVE-01`、`T-S1-PLUGIN-01`、`T-S1-TARGET-01`、`T-S1-TEST-01`

**Sample:** `scenarios/native_smoke.yaml`

**Report Schema:** `astra.scenario_report.v1`

```bash
cargo run -p astra-cli -- target validate Docs/samples/astra-vn-script/project.yaml --target nativevn-game
cargo run -p astra-cli -- test run scenarios/native_smoke.yaml --headless --report target/reports/stage1.yaml
cargo run -p astra-cli -- report explain target/reports/stage1.yaml
```

Expected report:

```yaml
schema: astra.scenario_report.v1
stage: stage1-enginecore
status: pass
hashes:
  state: "hash128"
  event: "hash128"
  presentation: "hash128"
checks:
- id: runtime.determinism
  status: pass
- id: save.load.replay
  status: pass
- id: plugin.descriptor_gate
  status: pass
- id: target.manifest
  status: pass
```

## Stage 2：Media + Package

**闭环：** cooked package 可读写，strict scenario runner 不忽略未知 VN action，Windows/Web 平台能力报告覆盖 decode/audio/renderer/storage/package source。Provider URI VFS 已落地；Stage 2 当前由 Migration 11 重新打开 Headless 完成口径。已有 CPU capture、AudioGraph meter 与 media tests 继续保留，但还要补统一测试 host、序列化物理输入、真实 PNG/WAV、全 Runtime test 收束、模型审查和真实平台 preflight。

**Test IDs:** `T-S2-PACKAGE-01`、`T-S2-VFS-01`、`T-S2-MEDIA-01`、`T-S2-MEDIA-05`、`T-S2-RUNTIME-FSM-01`、`T-S2-RUNTIME-AWAIT-01`、`T-S2-SCENARIO-GATE-01`、`T-S2-PLATFORM-01`、`T-S2-TARGET-GATE-01`、`T-S2-GATE-01`、`T-S2-WINDOWS-HOST-01`、`T-S2-WEB-HOST-01`

**Sample:** `Examples/NativeSmoke`

**Report Schema:** `astra.release_report.v1`

```bash
astra platform probe --platform windows --target native-smoke-game --report target/reports/platform-windows.yaml
astra package build target/cooked --target native-smoke-game --out target/native_smoke.astrapkg
astra package validate target/native_smoke.astrapkg --profile desktop-release --target native-smoke-game --platform-report target/reports/platform-windows.yaml --report target/reports/stage2.yaml
```

Expected report includes `package.integrity`, `target.manifest`, `platform.capability_report`, `renderer.headless_capture`, `decode.capability`, `audio.headless_meter`, `vfs.package_mount`, `vfs.local_authorized_mount`, `vfs.legacy_pack_mount`, `vfs.overlay_mount` and payload/path redaction diagnostics.

## Stage 3：AstraVN

**闭环：** `.astra + Luau policy` 编译为 CompiledStory，full playthrough 覆盖 commercial baseline、system UI、save/load 和 replay hash；advanced presentation profile 有独立 opt-in scenario。NativeVN 已由 RuntimeWorld `astra.vn.step` action 驱动，FFI instance/session lifecycle 和 package-bound behavior release evidence 已闭合。Stage 3 当前继续 `IN_PROGRESS`：现有 parser/compiler 已校验 canonical 缩进与 option 归属，但 `S3-SCRIPT-01` 和 `S3-SCRIPT-02` 仍需 Lexer、TokenStream、Lossless CST、Typed AST、Semantic Passes、Command Registry、token-level source map、formatter/LSP adapter 和 release conformance；Web live player 与 TsuiNoSora 正式 gate 也未完成。

**Test IDs:** `T-S3-SCRIPT-01`、`T-S3-SCRIPT-02`、`T-S3-UI-SCRIPT-01`、`T-S3-UI-EXT-01`、`T-S3-LUAU-01`、`T-S3-LUAU-02`、`T-S3-PRESENT-01`、`T-S3-SYSTEM-01`、`T-S3-ADVANCED-01`、`T-S3-SAMPLE-01`、`T-S3-GAME-TARGET-01`、`T-S3-RUNTIME-PROVIDER-01`

**Sample:** `Examples/NativeVN`、`Examples/NativeVN/scenarios/route_library.yaml`、`Examples/NativeVN/scenarios/route_rooftop.yaml`

**Report Schema:** `astra.scenario_report.v1` + `astra.release_report.v1`

```bash
astra package build target/cooked-nativevn --target nativevn-game --out target/nativevn.astrapkg
astra test run Examples/NativeVN/scenarios/route_library.yaml --package target/nativevn.astrapkg --target nativevn-game --profile advanced-vn --headless --report target/reports/stage3-library.yaml
astra cook Examples/NativeVN/project.yaml --profile advanced-vn --target nativevn-game --out target/cooked-nativevn-advanced
astra package build target/cooked-nativevn-advanced --target nativevn-game --out target/nativevn-advanced.astrapkg
astra test run Examples/NativeVN/scenarios/route_rooftop.yaml --package target/nativevn-advanced.astrapkg --target nativevn-game --profile advanced-vn --headless --report target/reports/stage3-rooftop.yaml
```

Expected report includes `target.manifest`, `runtime_provider.native_vn`, `script.compile`, `script.frontend_conformance`, `luau.policy_lock`, `system_stories.covered`, `vn.system_ui_profile`, `vn.advanced_presentation`, `command.provider_binding` and `vn.replay_hash`.

## Stage 4：Editor + AI/MCP

**闭环：** Creator 从 Project Wizard 到 Package/Release Gate 可用；AI/MCP 写入有 audit、diff 和 rollback。Runtime Director 通过受限 MCP session 调模型，角色记忆、Context Pack、provider profile、ONNX ModelBundle 和玩家 consent 通过 gate。Stage 4 当前按 `REOPENED_SPEC` 跟随 VFS/GameRuntime 调整：ONNX ModelBundle、Context Pack、generated artifact 和 MCP package access 必须复用 Asset VFS，不允许 loose sidecar 或私有 package source。

**Test IDs:** `T-S4-EDITOR-01`、`T-S4-PLUGIN-01`、`T-S4-EDITOR-04`、`T-S4-EDITOR-05`、`T-S4-EDITOR-TARGET-01`、`T-S4-AI-01`、`T-S4-AI-02`、`T-S4-AI-ONNX`、`T-S4-AI-VFS-01`、`T-S4-AI-03`、`T-S4-AI-04`、`T-S4-MCP-01`、`T-S4-MCP-02`、`T-S4-GATE-01`

**Sample:** `Examples/NativeVN` opened through Project Wizard

**Report Schema:** `astra.editor_report.v1`

```bash
cargo test -p astra-editor-bridge editor_creator_loop
cargo test -p astra-editor-bridge editor_target
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-ai runtime_ai_replay
cargo test -p astra-ai provider_profiles
cargo test -p astra-ai onnx_model_bundle
cargo test -p astra-ai runtime_memory
cargo test -p astra-mcp capability_protocol
cargo test -p astra-mcp context_tooling
cargo test -p astra-release ai_mcp_gate
```

Expected report: `astra.editor_report.v1` with source span links for failed checks, plus `astra.release_report.v1` checks for `ai.provider_profile`, `ai.model_bundle`, `ai.onnx_runtime_pack`, `ai.onnx_execution_provider`, `ai.model_bundle_vfs_mount`, `ai.generated_artifact_save`, `ai.runtime_memory_policy`, `mcp.context_permission` and provider-free replay.

## Stage 5：AstraEMU

**闭环：** Manager 启动 `AstraEmuRuntimeProvider` gameplay runtime，provider 创建并驱动 RuntimeWorld，再通过 `LegacyRuntimeProvider` family facade、auto probe、Trusted Luau、文本翻译、FilterGraph preset、legacy pack VFS mount 和 Artemis 通用 family plugin 通过 gate。其他 family 可以停在 alpha profile，但必须有 probe report。Stage 5 当前按 `REOPENED_SPEC` 对齐 peer gameplay runtime 与 EmulatorCore 状态机映射。

**Test IDs:** `T-S5-GAME-RUNTIME-01`、`T-S5-EMUCORE-SM-01`、`T-S5-LEGACY-VFS-01`、`T-S5-MANAGER-01`、`T-S5-MANAGER-UI-01`、`T-S5-PROGRAM-TARGET-01`、`T-S5-FAMILY-01`、`T-S5-ARTEMIS-01`、`T-S5-FVP-01`、`T-S5-GATE-01`

**Sample:** `scenarios/emu/artemis_full_flow.yaml` and local authorized case root

**Report Schema:** `astra.emu_case_report.v1`

```bash
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/stage5-artemis.yaml
astra emu probe <case-root> --family auto --report target/reports/emu-probe.yaml
```

Expected report omits commercial payload and contains `emu.game_runtime_provider`、`emu.legacy_runtime_provider`、`emu.vm_state_machine_trace`、`emu.legacy_pack_vfs`、`emu.auto_probe`、trusted script isolation、text redaction、filter preset evidence、trace、TextCaptureEvent、snapshot ref、redaction status and Runtime replay hash.

## Stage 6：Platform Completion

**闭环：** Linux、macOS、iOS 和 Android 分别提供真实 SDK、launcher/window、surface、platform decode、audio、save store、package source、resume 和 release evidence。Stage 6 只处理 Windows/Web 之外的平台完成，不改变 Stage 2 的 Windows/Web 完成边界。

**Test IDs:** `T-S6-LINUX-HOST-01`、`T-S6-MACOS-HOST-01`、`T-S6-IOS-HOST-01`、`T-S6-ANDROID-HOST-01`

**Sample:** `Examples/NativeVN` packaged as one Game target, then validated per platform.

**Report Schema:** `astra.platform_capability_report.v2` + `astra.platform_host_conformance_report.v1` + `astra.release_report.v1`

```bash
astra platform probe --platform linux --target nativevn-game --report target/reports/platform-linux.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game --platform-report target/reports/platform-linux.yaml --report target/reports/stage6-linux.yaml
```

Expected report includes `platform.capability_report`、`platform.host_conformance`、`platform.evidence_continuity`、package source/decode/save/resource lifecycle evidence；缺 provider、缺 conformance 或 identity 断裂必须 blocking。

## Stage 7：AstraRPG + `rpg.trpg` Profile

**闭环：** `AstraRpgRuntimeProvider` 作为新的 gameplay runtime provider 启动 `RpgSession`，通过 `RpgIntent`、`RpgEffect` 和 `CommittedAgentOutput` 驱动 RuntimeWorld；AI Town 20 NPC 一日 headless scenario 通过 save/load/replay hash；`rpg.trpg` profile 提供 ruleset、dice ledger、check/ruling ledger、seat authority 和 transcript redaction；CP2020 只作为 local-private adapter/sample gate，不提交规则正文、表格、商业 payload 或可复原内容。

**Test IDs:** `T-S7-POLICY-01`、`T-S7-RPG-PROVIDER-01`、`T-S7-RPG-CORE-01`、`T-S7-RPG-POLICY-01`、`T-S7-RPG-AI-TOWN-01`、`T-S7-RPG-TRPG-01`、`T-S7-RPG-CP2020-01`、`T-S7-RPG-GATE-01`

**Sample:** `Examples/AstraRPG/AITown`、`Examples/AstraRPG/CP2020LocalAdapter`

**Report Schema:** `astra.scenario_report.v1` + `astra.release_report.v1`

```bash
cargo test -p astra-policy
cargo test -p astra-rpg-core
cargo test -p astra-rpg-runtime-provider
cargo test -p astra-rpg-policy
cargo test -p astra-rpg-trpg
astra test run Examples/AstraRPG/AITown/Content/Scenarios/one_day_headless.yaml --target ai-town-headless --headless --report target/reports/ai-town.yaml
astra test run Examples/AstraRPG/CP2020LocalAdapter/Content/Scenarios/social_check.yaml --target cp2020-local-headless --headless --report target/reports/cp2020-local.yaml
cargo test -p astra-release rpg_gate
```

Expected report:

```yaml
schema: astra.release_report.v1
stage: stage7-astra-rpg
status: pass
checks:
- id: rpg.runtime_provider
  status: pass
- id: rpg.policy_bundle
  status: pass
- id: rpg.intent_validator
  status: pass
- id: rpg.provider_free_replay
  status: pass
- id: rpg.ai_town.twenty_npc_one_day
  status: pass
- id: rpg.trpg.dice_determinism
  status: pass
- id: rpg.trpg.seat_authority
  status: pass
- id: rpg.trpg.transcript_redaction
  status: pass
- id: rpg.cp2020.local_private_adapter
  status: pass
```

Stage 7 仍是 `SPEC_READY`。Rust crate、scenario 和 release check 必须在后续迁移中按 [AstraRPG design alignment migration](../migrations/astra-rpg-design-alignment-migration.md) 分步落地。

## Stage 8：AstraRPG Server/Client Protocol

**闭环：** AstraRPG Server/Client protocol 在 Stage 7 provider、save/replay、seat authority 和 transcript redaction 稳定后实现。Server/Client 只同步可序列化协议 envelope、seat state、intent/effect ack、transcript cursor 和 redacted audit，不传递 provider native handle、未提交 AI 输出、商业 payload、本地路径或未授权规则内容。

**Test IDs:** `T-S8-RPG-NET-CONTRACT-01`、`T-S8-RPG-NET-SERVER-01`、`T-S8-RPG-NET-CLIENT-01`、`T-S8-RPG-NET-REPLAY-01`

**Sample:** Stage 7 AI Town and `rpg.trpg` sessions run through local loopback protocol fixtures.

**Report Schema:** `astra.rpg_network_report.v1` + `astra.release_report.v1`

```bash
cargo test -p astra-rpg-net protocol_schema
cargo test -p astra-rpg-server server_session
cargo test -p astra-rpg-client client_session
cargo test -p astra-release rpg_network_gate
```

Expected report:

```yaml
schema: astra.rpg_network_report.v1
stage: stage8-astra-rpg-network
status: pass
checks:
- id: rpg.net.handshake
  status: pass
- id: rpg.net.seat_sync
  status: pass
- id: rpg.net.transcript_sync
  status: pass
- id: rpg.net.provider_free_replay
  status: pass
- id: rpg.net.redacted_audit
  status: pass
```

Stage 8 仍是 `SPEC_READY`，只依赖 Stage 7 的稳定 public contract。

## v1 Gate

全系列 v1 同时要求：

- EngineCore、AstraVN、AstraEditor、AstraPlatform、AstraEMU 都有 release profile。
- Windows、Linux、macOS、iOS、Android、Web 通过对应 profile gate。
- AstraEMU v1 的可用 family 是 Artemis；其他 family 输出 alpha scaffold report；AstraEMU family 默认 in-process，外部 bridge 只作为普通 extension point。
- AstraRPG Stage 7 和 AstraRPG Server/Client Stage 8 是 v1 后续扩展，不作为当前 v1 release gate。

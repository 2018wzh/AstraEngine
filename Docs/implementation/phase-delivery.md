# Phase Delivery

全系列 v1 按 Stage 1-5 推进。每个 Stage 都要产出可运行闭环、固定命令和 machine-readable report；不能只交付内部库。

## Stage 1：EngineCore

**闭环：** native smoke package 在 headless Runtime 中启动、推进、save/load、replay，并通过 plugin descriptor gate。

**Test IDs:** `T-S1-BOOT-01`、`T-S1-SAVE-01`、`T-S1-PLUGIN-01`、`T-S1-TEST-01`

**Sample:** `scenarios/native_smoke.yaml`

**Report Schema:** `astra.scenario_report.v1`

```bash
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
```

## Stage 2：Media + Package

**闭环：** cooked package 可读写，headless capture 稳定，平台能力报告覆盖 decode/audio/renderer。

**Test IDs:** `T-S2-PACKAGE-01`、`T-S2-MEDIA-01`、`T-S2-MEDIA-05`、`T-S2-GATE-01`

**Sample:** `Examples/NativeSmoke`

**Report Schema:** `astra.release_report.v1`

```bash
astra package build Examples/NativeSmoke --out target/native_smoke.astrapkg
astra package validate target/native_smoke.astrapkg --profile desktop-release --report target/reports/stage2.yaml
```

Expected report includes `package.integrity`, `renderer.headless_capture`, `decode.capability`, `audio.headless_meter`.

## Stage 3：AstraVN

**闭环：** `.astra + Luau policy` 编译为 CompiledStory，full playthrough 覆盖 system stories、save/load 和 replay hash。

**Test IDs:** `T-S3-SCRIPT-01`、`T-S3-LUAU-01`、`T-S3-LUAU-02`、`T-S3-SAMPLE-01`

**Sample:** `Examples/NativeVN`、`scenarios/full_playthrough.yaml`

**Report Schema:** `astra.scenario_report.v1` + `astra.release_report.v1`

```bash
astra package build Examples/NativeVN --out target/nativevn.astrapkg
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/reports/stage3.yaml
```

Expected report includes `script.compile`, `luau.policy_lock`, `system_stories.covered`, `vn.replay_hash`.

## Stage 4：Editor + AI/MCP

**闭环：** Creator 从 Project Wizard 到 Package/Release Gate 可用；AI/MCP 写入有 audit、diff 和 rollback。

**Test IDs:** `T-S4-EDITOR-01`、`T-S4-EDITOR-04`、`T-S4-EDITOR-05`、`T-S4-AI-02`、`T-S4-MCP-01`

**Sample:** `Examples/NativeVN` opened through Project Wizard

**Report Schema:** `astra.editor_report.v1`

```bash
cargo test -p astra-editor-bridge editor_creator_loop
cargo test -p astra-ai trusted_session_audit
cargo test -p astra-mcp capability_protocol
```

Expected report: `astra.editor_report.v1` with source span links for failed checks.

## Stage 5：AstraEMU

**闭环：** Manager/core IPC、shared memory、family API 和 Artemis 通用 compat core 通过 full-flow gate。其他 family 可以停在 alpha profile，但必须有 probe report。

**Test IDs:** `T-S5-MANAGER-01`、`T-S5-CORE-01`、`T-S5-ARTEMIS-01`、`T-S5-GATE-01`

**Sample:** `scenarios/emu/artemis_full_flow.yaml` and local authorized case root

**Report Schema:** `astra.emu_case_report.v1`

```bash
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/stage5-artemis.yaml
astra emu probe <case-root> --family auto --report target/reports/emu-probe.yaml
```

Expected report omits commercial payload and contains trace、TextCaptureEvent、snapshot ref、redaction status.

## v1 Gate

全系列 v1 同时要求：

- EngineCore、AstraVN、AstraEditor、AstraPlatform、AstraEMU 都有 release profile。
- Windows、Linux、macOS、iOS、Android、Web 通过对应 profile gate。
- AstraEMU v1 的可用 family 是 Artemis；其他 family 输出 alpha scaffold report。

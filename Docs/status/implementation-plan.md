# Implementation Plan Status

本页只记录当前代码完成度和下一步实施顺序。设计规格放在 `Docs/product`、`Docs/contracts`、`Docs/modules` 和 `Docs/implementation`；本页不把已规划内容写成已实现能力。

## 状态规则

| Status | 含义 | 允许标记条件 |
| --- | --- | --- |
| `DONE` | 已实现并有可运行证据 | 目标路径存在，关联测试通过，状态页写明证据命令 |
| `IN_PROGRESS` | 正在实现 | 已有代码落地，但关联 gate 未全部通过 |
| `SPEC_READY` | 设计可开工 | 文档、schema、gate 和测试映射已写清，代码未落地 |
| `RESEARCH_READY` | 研究资料可用于实现 | 仅限 AstraEMU family 研究和 probe 工具，不代表 runtime 已实现 |
| `NOT_STARTED` | 尚未开工 | 目标 crate、scenario 或 gate 仍不存在 |

以后实现某个工作项时，同步改本页、对应 `stage-*.md`、[stage-test-matrix.md](stages/stage-test-matrix.md) 和 [coverage-matrix.md](coverage-matrix.md)。没有测试或 release report 证据，不标 `DONE`。

## 当前代码快照

| Area | Code status | Evidence |
| --- | --- | --- |
| Stage 1 EngineCore | `DONE` | `cargo test --workspace` 通过；覆盖 core、runtime、plugin、property、headless scenario |
| Stage 2 Media + Package | `NOT_STARTED` | `astra-asset`、`astra-package`、`astra-media`、`astra-release` crate 尚不存在 |
| Stage 3 AstraVN | `SPEC_READY` | `.astra`、Luau、presentation、standard commands、system UI 和 advanced sample 已写入文档；`astra-vn` crate 尚不存在 |
| Stage 4 Editor + AI/MCP | `SPEC_READY` | Editor workflow、Plugin Manager、AI/MCP gate 已写入文档；`Editor/Source` 尚不存在 |
| Stage 5 AstraEMU | `SPEC_READY` | `Docs/contracts/astraemu-ipc.md`、`Docs/implementation/astraemu-legacy-runtime-framework.md` 和 `Docs/emu` 已写清 `LegacyRuntimeProvider` facade；`AstraEMU/Source` 尚不存在 |
| Six platforms | `SPEC_READY` | platform host/profile gate 已写入文档；平台 host crate 尚不存在 |

## Stage 1 完成项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S1-BOOT-01` | `DONE` | workspace、toolchain、CI 配置存在 |
| `S1-CORE-01` | `DONE` | `cargo test -p astra-core core_types` |
| `S1-RUNTIME-01` | `DONE` | `cargo test -p astra-runtime world_actor` |
| `S1-RUNTIME-02` | `DONE` | `cargo test -p astra-runtime state_machine_tick` |
| `S1-RUNTIME-04` | `DONE` | `cargo test -p astra-runtime state_machine_tick` and `cargo test -p astra-runtime delayed_event` |
| `S1-RUNTIME-03` | `DONE` | `cargo test -p astra-runtime await_token` |
| `S1-SAVE-01` | `DONE` | `cargo test -p astra-runtime save_replay` |
| `S1-PLUGIN-01` | `DONE` | `cargo test -p astra-plugin descriptor_gate` and `cargo test -p astra-plugin load_unload` |
| `S1-PLUGIN-02` | `DONE` | `cargo test -p astra-plugin ffi_action_provider` |
| `S1-PROP-01` | `DONE` | `cargo test -p astra-property --test property_metadata` and `cargo test -p astra-property --test expand_smoke` |
| `S1-TEST-01` | `DONE` | `cargo test -p astra-test native_smoke` |
| `S1-OBS-01` | `DONE` | `cargo test -p astra-cli --test logging` |

## 下一步实施顺序

| Order | Work | Status | Why now |
| --- | --- | --- | --- |
| 1 | `S2-PACKAGE-01` package container | `NOT_STARTED` | Stage 3/4/5 都需要统一 package/save section |
| 2 | `S2-ASSET-01` + `S2-ASSET-02` asset/import/cook | `NOT_STARTED` | package 需要 asset registry、sidecar 和 cook artifact |
| 3 | `S2-GATE-01` release report | `NOT_STARTED` | 后续 VN、Editor、EMU 都依赖 machine-readable gate |
| 4 | `S2-MEDIA-01` 到 `S2-MEDIA-05` media providers | `NOT_STARTED` | AstraVN presentation 和 AstraEMU family presentation/audio effect 的共同前置 |
| 5 | `S3-SCRIPT-01` + `S3-SCRIPT-02` `.astra` parser/compiler | `SPEC_READY` | AstraVN Core 和 Editor visual model 的前置 |
| 6 | `S3-PRESENT-01` + `S3-SYSTEM-01` | `SPEC_READY` | 新增设计要求 commercial baseline 和 system UI 阻断普通发布 |
| 7 | `S4-PLUGIN-01` Plugin Manager | `SPEC_READY` | 新插件设计需要 enablement、dependency graph 和 extension diagnostics |
| 8 | `S5-MANAGER-01` + `S5-FAMILY-01` | `SPEC_READY` | AstraEMU 已改为 RuntimeWorld + `LegacyRuntimeProvider` facade 架构 |

## 验证命令

```bash
python Tools/check_docs.py
cargo test --workspace
git diff --check
```

Expected output: docs check reports checked markdown files；workspace tests pass；diff check has no whitespace errors。

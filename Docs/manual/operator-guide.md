# Runtime / Platform Operator Guide

Operator 负责构建、打包、平台适配、Release Gate、crash bundle 和 AstraEMU local case report。

## 发布命令

```bash
astra target validate project.yaml --target nativevn-game
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra cook project.yaml --profile desktop-release --target nativevn-game --out target/cooked
astra package build target/cooked --target nativevn-game --out target/game.astrapkg
astra package validate target/game.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
astra test run scenarios/full_playthrough.yaml --package target/game.astrapkg --target nativevn-game --headless
```

## 日志命令

`astra` 默认把 machine-readable report 写到 stdout，把日志写到 stderr。需要结构化日志时使用：

```bash
astra test run scenarios/native_smoke.yaml --headless --format json --log-format json --log-filter astra_runtime=debug,astra_test=debug,astra_plugin=debug
```

需要落盘时显式传入相对目录；未传目录不会创建日志文件：

```bash
astra test run scenarios/native_smoke.yaml --headless --log-dir target/logs --log-max-file-bytes 16777216 --log-max-archives 8 --crash-dir target/crashes
```

日志只用于排障，不参与 replay、hash、save 或 release 判定。JSON file/ring 使用 `astra.log_event.v1`；低级别异步写入发生背压时，critical path 会写 `observability.queue.saturated` 和累计 `dropped_count`。禁止把商业正文、payload、secret、绝对路径或未筛选的对象 dump 写进日志。

Windows shipping Player 默认使用平台 writable `Saved/Logs` 与 `Saved/Crashes`，默认级别为 WARN。bundle 内的 crash reporter 必须通过 manifest hash、自检和启动握手；helper 缺失或被篡改会阻断启动。crash bundle 最多保留 10 份，按敏感本地产物处理，不要提交、打包或上传。Web 只有 console/ring/error tail，没有本地文件或 minidump。

## 平台能力报告

每个平台模块必须输出 renderer、decode、audio、filesystem、input、save persistence、network 和 AI permission capability。Release Gate 根据 profile 判断是否可发布。

缺少对应 SDK 时，platform report 必须写 `sdk_status: missing`。普通 CI 可以保留 schema 和 CLI 证据，但不能把该平台 release 标成完成。

## Report Reference

| Report | 用途 |
| --- | --- |
| `astra.release_report.v1` | 发布资格 |
| `astra.scenario_report.v1` | 无头玩家流程 |
| `astra.target_validation_report.v1` | Editor/Game/Program target |
| `astra.platform_capability_report.v2` | declared/available/selected 平台 provider |
| `astra.platform_host_conformance_report.v1` | build/profile/package/session 绑定的真实 host 生命周期证据 |
| `astra.plugin_report.v1` | 插件加载、卸载和 provider |
| `astra.emu.local_case_report.v1` | AstraEMU Artemis 和后续 family |

Stage 2 的 `astra package validate` 已输出 `astra.release_report.v1`，覆盖 package integrity、section bounds/hash、cook/project artifact、provider policy、media fallback policy、scenario refs、platform eligibility 和 platform report。`desktop-release`/`web-release` 缺 `compiled.project` 或 platform report 时阻断；headless/dev profile 的 platform report 可 warning。FFmpeg fallback 是 optional feature；profile 必须把缺失 FFmpeg 写成 warning 或 blocking。Release Gate check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。

ONNX Runtime local AI 发布时，operator 需要把 ModelBundle 当作 package 资产处理。模型、tokenizer、reduced runtime、Web runtime adapter 和 custom op sidecar 必须通过 cook/package 写入 Asset VFS section，并按 profile 绑定目标平台。Release Gate 校验 `ai.model_bundle`、`ai.model_bundle_vfs_mount`、`ai.onnx_runtime_pack`、`ai.onnx_execution_provider` 和 `ai.generated_artifact_save`；Windows、Linux、macOS/iOS、Android、Web 分别要求 `DirectML`、`OpenVINO`、`CoreML`、`QNN`、`WebNN` 主 EP 的真实目标运行证据。CPU fallback、release 阶段联网拉取 runtime、loose sidecar 或模型 payload 路径泄露都是 blocking diagnostic。
正式 Migration 8 evidence 使用 `python Tools/run_platform_host_acceptance.py ...` 汇总。脚本拒绝 dirty worktree，重新执行 Windows/Chrome host 测试，并校验两端 capability、conformance 与 Player report 的 package/profile/build/session continuity；输出 manifest 只包含 commit、hash、provider、check count、状态和 diagnostic，不包含输入文件路径。

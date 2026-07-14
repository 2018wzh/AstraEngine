# Runtime / Platform Operator Guide

Operator 负责构建、打包、平台适配、Release Gate、crash bundle 和 AstraEMU local case report。

## Checkout-bound Cargo 验证

多 worktree 或本地 release 验证统一通过 checkout-bound 入口执行：

```bash
python Tools/run_cargo_isolated.py clippy --workspace --all-targets -- -D warnings
python Tools/run_cargo_isolated.py test --workspace
```

该入口从 commit/dirty state、workspace manifest、Cargo.lock、Rust toolchain 和 feature/target/profile 参数派生独立 target root。target root 内的 `astra-build-identity.json` 使用 `astra.build_identity.v1`，只记录 hash、状态、artifact role、相对路径和 byte size。报告损坏、identity 不一致、动态 fixture 不在同一 target root 或 Cargo 返回非零状态时必须停止，不得改用共享 `target/debug` 继续生成证据。

性能验收必须把同一份 build identity 继续传入 `PerformanceRunIdentity`，并补齐 package hash、host profile hash、product profile 和 session id。`astra.performance_report.v1` 为 `blocked` 时，应按 diagnostic 检查 run duration、sample count、threshold 或 identity drift；不要重写报告、删掉慢 sample或在采样后放宽 budget。正式校验使用 `ReleaseValidator::validate_package_with_product_evidence`，同时提交 capability、conformance、Player、budget 和 report。普通 debug test 只验证 recorder、host 与 validator 接线，正式阈值需要 release build 与声明的参考环境。

## 发布命令

```bash
astra target validate project.yaml --target nativevn-game
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra cook project.yaml --profile desktop-release --target nativevn-game --out target/cooked
astra package build target/cooked --target nativevn-game --out target/game.astrapkg
astra package validate target/game.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
astra test run scenarios/full_playthrough.yaml --package target/game.astrapkg --target nativevn-game --headless
```

上面的 `--headless` 是当前既有入口。Migration 11 已固化 typed launch/profile contract，但尚未提供完整 `astra-headless` host/binary。迁移完成后，旧 flag 将返回明确错误，不保留隐式 alias。

## Planned Headless Platform workflow

Migration 11 完成后的 Developer 入口为独立 binary：

```bash
astra-headless run \
  --profile tests/headless/profile.json \
  --input tests/headless/full-playthrough.jsonl \
  --artifacts target/headless/full-playthrough

astra-headless serve --stdio \
  --profile tests/headless/profile.json \
  --artifacts target/headless/interactive
```

文件与 stdio 使用同一双向 JSONL 协议。默认保存全部 presented frame PNG 和完整 PCM S16LE WAV；`all`、`checkpoints`、`final`、`manifest-only` 必须显式受 frame、byte、duration 和 artifact count 限额约束。stdout 只输出协议或 report，日志只写 stderr。

产品、Player、样例或 full-playthrough 必须先通过自动比较，再由模型查看 required checkpoint、首尾帧、最大差异帧和失败邻近帧。音频要检查 WAV、波形、频谱、响度、静音、削波、声道和时长；涉及语音内容或音画同步时还要试听。模型不能覆盖自动失败或自行放宽容差。

真实平台验收只能在 `astra.headless_run_report.v1`、`astra.headless_review.v1` 和 `astra.headless_preflight_link.v1` 全部通过后启动。Headless 与真实平台 run 必须绑定同一 build、cooked package、input sequence、scenario、target 和 content identity；Headless 结果不能替代真实窗口、浏览器、音频设备或原生输入证据。

会渲染文本的 shipping profile 必须在 `media.manifest` 中设置 `font_manifest_required: true`，并通过 `font_manifest_section` 指向同包内的 `astra.font_manifest.v1`。字体 manifest 的每个条目必须绑定 package VFS URI、provider、target/profile、face index、license、coverage 和内容 hash。验证器不会读取系统字体或 loose file 补齐缺失资源；`media.font_package` blocked 时应修复 package/cook 输入，不能关闭检查或改成 optional。

NativeVN 字体 asset 的 `astra.asset.v1` sidecar 必须声明 `font.family`、`font.face_index`、可选 `font.subset` 和有序且不重叠的 Unicode scalar `font.coverage`。项目通过 `nativevn.default_locale` 选择默认语言；Cook 只从当前 target/profile 可用的 `vn.localization.<locale>` sections 生成 `player.locale_config`。Release Gate 会重新读取 config 和每个声明的 localization section，并阻断缺 section、重复 key、locale/schema 漂移或 default 不在 available 列表的 package；不得由 Player 在运行时读取 loose JSON 或回退到硬编码语言。

Windows 字形视觉回归由 `astra.windows_gpu_glyph_golden.v1` 绑定字体 revision、layout hash 和真实 GPU capture hash。更新字体、shaping provider 或 atlas shader 后，必须先确认视觉变化符合设计，再在同一变更中更新 golden；不能只改 hash 让测试通过。`platform-test-driver` 的 device-loss 注入只用于自动验证 retained glyph resource rebuild，正式发布证据仍需记录真实 host、build、profile、package 和 session identity。Web text pass 尚未实现时必须返回 `PLATFORM_NOT_IMPLEMENTED`，不能改用 headless capture 代替。

产品 release evidence 还必须提供 `astra.player_presentation_report.v1`。该报告只能由 Player command sink 完成真实平台 capture 后生成，并与 capability、host conformance、automation 的 package/profile/build/session identity 一致。缺报告、`astra.renderer.headless`、零变化像素或 identity drift 都是 `ASTRA_PLAYER_PRESENTATION_EVIDENCE` blocking；不能手工填写静态 hash 补过门禁。

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
| `astra.headless_artifact_manifest.v1` | planned Headless PNG/WAV 相对路径、hash、尺寸、时长和 provider identity |
| `astra.headless_run_report.v1` | planned 平台无关 host、输入、产物与自动比较结果 |
| `astra.headless_review.v1` | planned 模型视觉/音频审查结果 |
| `astra.headless_preflight_link.v1` | planned Headless 与真实平台 run 的 identity 关联 |
| `astra.plugin_report.v1` | 插件加载、卸载和 provider |
| `astra.emu.local_case_report.v1` | AstraEMU Artemis 和后续 family |

Stage 2 的 `astra package validate` 已输出 `astra.release_report.v1`，覆盖 package integrity、section bounds/hash、cook/project artifact、provider policy、media fallback policy、scenario refs、platform eligibility 和 platform report。`desktop-release`/`web-release` 缺 `compiled.project` 或 platform report 时阻断；headless/dev profile 的 platform report 可 warning。FFmpeg fallback 是 optional feature；profile 必须把缺失 FFmpeg 写成 warning 或 blocking。Release Gate check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。

ONNX Runtime local AI 发布时，operator 需要把 ModelBundle 当作 package 资产处理。模型、tokenizer、reduced runtime、Web runtime adapter 和 custom op sidecar 必须通过 cook/package 写入 Asset VFS section，并按 profile 绑定目标平台。Release Gate 校验 `ai.model_bundle`、`ai.model_bundle_vfs_mount`、`ai.onnx_runtime_pack`、`ai.onnx_execution_provider` 和 `ai.generated_artifact_save`；Windows、Linux、macOS/iOS、Android、Web 分别要求 `DirectML`、`OpenVINO`、`CoreML`、`QNN`、`WebNN` 主 EP 的真实目标运行证据。CPU fallback、release 阶段联网拉取 runtime、loose sidecar 或模型 payload 路径泄露都是 blocking diagnostic。
正式 Migration 8 evidence 使用 `python Tools/run_platform_host_acceptance.py ...` 汇总。脚本拒绝 dirty worktree，重新执行 Windows/Chrome host 测试，并校验两端 capability、conformance 与 Player report 的 package/profile/build/session continuity；输出 manifest 只包含 commit、hash、provider、check count、状态和 diagnostic，不包含输入文件路径。

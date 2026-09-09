# Runtime / Platform Operator Guide

Operator 负责构建、打包、平台适配、Release Gate 与 crash bundle；AstraEMU 使用下述独立 Host 流程。

## Worktree 内 Cargo 验证

每次验证都应在当前实例独占的 worktree 中直接执行 Cargo。workspace test 依赖 Headless test driver，因此先构建对应 binary：

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p astra-headless
cargo test --workspace
cargo build -p astra-headless --features ffmpeg-vcpkg
cargo test --workspace --features ffmpeg-vcpkg
```

禁止让多个实例共享 worktree 或 `CARGO_TARGET_DIR`。Headless 测试框架只解析当前 Cargo profile 中的 `astra-headless`，为测试进程生成临时 profile、package、build identity 和 artifact root，并在最后一个 session 结束后删除这些产物。binary 缺失、hash 不一致、bootstrap 失败或清理失败都会阻断测试。

任务结束后，应停止当前实例启动的进程，删除不再需要的临时报告、fixture 和构建缓存。只允许清理当前实例拥有的产物；移除 worktree 前必须确认没有未提交修改，也不能触碰其他实例仍在使用的目录。

Windows 的 `ffmpeg-vcpkg` job 要求设置 `VCPKG_ROOT`，并把 `VCPKG_DEFAULT_TRIPLET` 对应的 release/debug runtime 目录显式加入 `PATH`。目录或 runtime 缺失时命令直接阻断，不复制 DLL，也不退回无视频模式。

CI 的默认 Headless job 执行 docs、fmt、clippy、Headless driver build、workspace test、test convergence 与 shipping graph 检查。独立 Windows job 从显式 vcpkg root 安装 FFmpeg，并以 `ffmpeg-vcpkg` 同时运行 workspace clippy/test；配置存在不等于 job 已通过，状态页只能引用实际 CI run evidence。

性能验收必须把同一份 build identity 继续传入 `PerformanceRunIdentity`，并补齐 package hash、host profile hash、product profile 和 session id。`astra.performance_report.v1` 为 `blocked` 时，应按 diagnostic 检查 run duration、sample count、threshold 或 identity drift；不要重写报告、删掉慢 sample或在采样后放宽 budget。正式校验使用 `ReleaseValidator::validate_package_with_product_evidence`，同时提交 capability、conformance、Player、budget 和 report。普通 debug test 只验证 recorder、host 与 validator 接线，正式阈值需要 release build 与声明的参考环境。

## 发布命令

```bash
astra target validate project.yaml --target nativevn-game
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra cook project.yaml --profile desktop-release --target nativevn-game --out target/cooked
astra package build target/cooked --target nativevn-game --out target/game.astrapkg
astra package validate target/game.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
astra-headless run --profile tests/headless/profile.json --package target/game.astrapkg --input tests/headless/full-playthrough.jsonl --artifact-root target/headless/full-playthrough --build-identity target/identity/astra-build-identity.json
```

旧 `astra test run --headless` 已退役并返回 `ASTRA_TEST_HEADLESS_MIGRATED`；它不再读取 YAML、不转发，也不保留隐式 alias。

## Headless Platform workflow

Migration 11 的 Developer 入口为独立 binary：

```bash
astra-headless run \
  --profile tests/headless/profile.json \
  --package target/game.astrapkg \
  --input tests/headless/full-playthrough.jsonl \
  --artifact-root target/headless/full-playthrough \
  --build-identity target/identity/astra-build-identity.json

astra-headless serve --stdio \
  --build-identity target/identity/astra-build-identity.json

astra-headless prepare-review \
  --run-report target/headless/full-playthrough/run-report.json \
  --manifest target/headless/full-playthrough/artifact-manifest.json \
  --artifact-root target/headless/full-playthrough \
  --output target/headless/full-playthrough/review-bundle.json

astra-headless validate-review \
  --run-report target/headless/full-playthrough/run-report.json \
  --bundle target/headless/full-playthrough/review-bundle.json \
  --review target/headless/full-playthrough/review.json
```

文件与 stdio 使用同一双向 JSONL 协议。普通功能运行可显式迁移 Headless profile v2；正式性能运行只接受 v3。`render_policy: checkpoints` 会校验所有 scene 并写入 submitted hash，仅首帧、具名 checkpoint 和末帧产生 RGBA；逐帧视觉验证显式使用 `all`。`max_submitted_frames` 与 `max_rasterized_frames` 任一超限都会阻断。stdout 只输出协议或 report，日志只写 stderr。

GPU job 必须先把 profile renderer 绑定为 `wgpu_offscreen`，再给 `run` 或 `serve` 传 `--gpu`；CPU profile 与 flag 混用会阻断。workspace test job 通过 `ASTRA_HEADLESS_GPU=1 cargo test ...` 让 `HeadlessTestContext` 生成 GPU profile 并启动 `serve --gpu`。该模式固定 Windows/DX12、Linux/Vulkan、macOS/Metal；Windows DX12 使用 build-locked static DXC，不读取 PATH 中的动态 DXC。所有平台都要求 hardware adapter，不回退软件 adapter或其他 backend。

正式性能运行在 v3 profile 的 `gpu_adapter` 中精确声明 backend、device type 和 `require_timestamp_query: true`，同时设置 `presentation_rate_hz: 120`。正式参考机还要把探测 run 的 `adapter_identity_hash` 原样写入 profile；该值与 run report、artifact manifest 和 performance trace manifest 使用同一 `RendererExecutionIdentity` canonical hash，不能换算或手写另一套设备指纹。Runtime 仍以 60 Hz 固定步长推进，Headless 只把 presentation 分成两个 substep；普通 v2/v3 功能 profile 保持 60 Hz。`astra-headless performance-e2` 只接受 clean Release build、匹配的 package/budget/profile/build identity，并固定执行 1,200 帧 warmup 与 72,000 帧 measurement。产品 workload 通过 `run --performance-budget --performance-report --performance-trace --performance-trace-manifest --performance-start-sequence` 复用同一 package bootstrap 和物理输入，不能用 synthetic report 替代产品路线证据。

先从已校验的普通 Headless profile 派生性能 profile，再由工具写入固定阈值预算。不要手写或在运行后修改 JSON：

```bash
astra-headless prepare-performance-profile \
  --input profiles/headless.json \
  --output evidence/profile-integrated.json \
  --backend dx12 \
  --device-type integrated

astra-headless prepare-performance-budget \
  --profile evidence/profile-integrated.json \
  --output evidence/scene2d-budget.json \
  --budget-id scene2d.120hz.run-1 \
  --kind renderer-stress

astra-headless performance-e2 \
  --profile evidence/profile-integrated.json \
  --package evidence/product.astrapkg \
  --budget evidence/scene2d-budget.json \
  --report evidence/scene2d-report.json \
  --trace evidence/scene2d-trace.json \
  --trace-manifest evidence/scene2d-trace-manifest.json \
  --build-identity evidence/build-identity.json \
  --workload scene2d1080p \
  --run-index 1
```

800×600 产品压力使用 `product-stress`，完整路线使用 `product-route`，并给 `run` 传对应的 `--performance-*` 参数。先用 `prepare-product-performance-input` 读取已经验收的 Title 导航前缀，再生成 1,200 + 72,000 个 presentation sample；生成器会保留物理输入与 `Await` 的真实 tick 语义。压力运行还必须传 `--performance-warmup-frames 1200`，并把生成器返回的 start sequence 原样传给 `run`。完整路线不伪造 72,000 个输入 sample，而是按实际 GPU submission 数生成固定预算。三次集显压力报告必须全部通过。独显 profile 和报告放在单独对照目录，不能参与 release decision。

Trace 写入 ignored 目录后，Codex 通过外部 `perfetto-mcp==0.1.4` 的 `find_slices` 和 `execute_sql_query` 查看热点。不要把第三方 MCP 放进仓库，也不要修改 `astra-mcp`。设备名、本地路径和原始 trace 不进入文档、package 或 report。性能报告只证明所声明 adapter、workload 和 E2 identity；Windows E3 仍需另行验收。

产品、Player、样例或 full-playthrough 必须先通过自动比较，再运行 `prepare-review`。模型或具名人工只能按 bundle 查看 required checkpoint、首尾帧、最大差异帧、失败邻近帧和完整 WAV，不得自行省略条目。音频要检查波形、频谱、响度、静音、削波、声道和时长；涉及语音内容或音画同步时还要试听。完成的 `astra.headless_review.v2` 必须再通过 `validate-review`；模型不能覆盖自动失败或自行放宽容差。

checkpoint 未显式改写时使用固定的受控宽松默认容差。任何自定义容差都要在 config 中绑定 `astra.headless_tolerance_approval.v2` 的相对路径和 SHA-256；approval 只能是具名人工，必须匹配 tolerance-set hash。`astra-headless` 会把完整 checkpoint config hash 写入新 run report。修改 approval、config 或 baseline 后必须重跑，不能复用或编辑旧 report。

真实平台验收只能在 `astra.headless_run_report.v2`、`astra.headless_review_bundle.v2` 和 `astra.headless_review.v2` 全部通过后启动。平台 automation 完成后输出 `astra.platform_run_identity.v1`，再运行 `astra-headless link-preflight --headless-run-report ... --platform-run-identity ... --output ...`。Headless 与真实平台 run 必须绑定同一 build、cooked package、input sequence、scenario、target 和 content identity；`astra.headless_preflight_link.v2` 只建立关联，Headless 结果不能替代真实窗口、浏览器、音频设备或原生输入证据。

正式 Windows/Web 联合验收统一走 `Tools/run_platform_host_acceptance.py`。该入口在启动任何真实 host 命令前先校验 Headless run、review bundle、review、两份 platform run identity 和两份 preflight link；自动失败、review verdict 缺失、artifact hash 漂移或任一 identity 不一致都会在 host 启动前阻断。`--skip-host-runs` 只用于复核已经形成的同 run 证据，不能生成 E3：

```bash
python Tools/run_platform_host_acceptance.py \
  --package target/product/game.astrapkg \
  --headless-run-report target/headless/run-report.json \
  --headless-review-bundle target/headless/review-bundle.json \
  --headless-review target/headless/review.json \
  --windows-platform-run-identity target/windows/platform-run-identity.json \
  --windows-preflight-link target/windows/headless-preflight-link.json \
  --web-platform-run-identity target/web/platform-run-identity.json \
  --web-preflight-link target/web/headless-preflight-link.json \
  --windows-capability target/windows/capability.json \
  --windows-conformance target/windows/conformance.json \
  --windows-player target/windows/player.json \
  --web-capability target/web/capability.json \
  --web-conformance target/web/conformance.json \
  --web-player target/web/player.json \
  --out target/platform-acceptance.json
```

会渲染文本的 shipping profile 必须在 `media.manifest` 中设置 `font_manifest_required: true`，并通过 `font_manifest_section` 指向同包内的 `astra.font_manifest.v1`。字体 manifest 的每个条目必须绑定 package VFS URI、provider、target/profile、face index、license、coverage 和内容 hash。验证器不会读取系统字体或 loose file 补齐缺失资源；`media.font_package` blocked 时应修复 package/cook 输入，不能关闭检查或改成 optional。

NativeVN 字体 asset 的 `astra.asset.v1` sidecar 必须声明 `font.family`、`font.face_index`、可选 `font.subset` 和有序且不重叠的 Unicode scalar `font.coverage`。项目通过 `nativevn.default_locale` 选择默认语言；Cook 只从当前 target/profile 可用的 `vn.localization.<locale>` sections 生成 `player.locale_config`。Release Gate 会重新读取 config 和每个声明的 localization section，并阻断缺 section、重复 key、locale/schema 漂移或 default 不在 available 列表的 package；不得由 Player 在运行时读取 loose JSON 或回退到硬编码语言。

Windows 字形视觉回归由 `astra.windows_gpu_glyph_golden.v1` 绑定字体 revision、layout hash 和真实 GPU capture hash。更新字体、shaping provider 或 atlas shader 后，必须先确认视觉变化符合设计，再在同一变更中更新 golden；不能只改 hash 让测试通过。`platform-test-driver` 的 device-loss 注入只用于自动验证 retained glyph resource rebuild，正式发布证据仍需记录真实 host、build、profile、package 和 session identity。Web text pass 尚未实现时必须返回 `PLATFORM_NOT_IMPLEMENTED`，不能改用 headless capture 代替。

产品 release evidence 还必须提供 `astra.player_presentation_report.v1`。该报告只能由 Player command sink 完成真实平台 capture 后生成，并与 capability、host conformance、automation 的 package/profile/build/session identity 一致。缺报告、`astra.renderer.headless`、零变化像素或 identity drift 都是 `ASTRA_PLAYER_PRESENTATION_EVIDENCE` blocking；不能手工填写静态 hash 补过门禁。

## AstraEMU 独立 Host

AstraEMU 首轮提供 Windows、Slint Manager 与 FVP。构建、插件加载和游戏运行不再使用 Engine product package、签名 Family manifest、旧 CLI、Headless report 或 RuntimeWorld。旧 Android/iOS 发布流程暂不适用于本轮。

Manager 从本地安装的动态库读取独立 Family ABI descriptor，校验 ABI 和 capability；多个 probe 命中时由用户选择。游戏由 Family 直接读取原生文件和管理存档。资料库与设置使用新 schema，旧 Manager 数据重建，原生存档不迁移也不由 Host 改写。

独立 Host 实现仍在本次重构中；具体命令与 Windows 游戏验证结果在集成完成后更新。本轮计划与边界见 [独立 Host 重构](../migrations/astraemu-independent-host.md)，当前进度见 [Stage 5](../status/stages/stage-5-astra-emu.md)。不生成或保留新的 EMU evidence/report 体系。

## AstraEMU 兼容性数据仓

AstraEMU 社区兼容性库是独立维护的只读数据仓，经 GitHub Pages 托管为静态 JSON（schema `astra.emu.compatibility.v2`），不进入本仓 workspace、依赖图或 release gate。数据仓 CI 用 `astra-emu-metadata` 的 `compatibility_json_schema()` 导出的 JSON Schema 校验文档；Rust 类型是 schema 真源，修改分级或字段必须先改 `compatibility.rs` 再重新导出。格式与边界见 [Data Formats](../contracts/data-formats.md) 的社区兼容性库节。

v2 让兼容性由 **VNDB 一键控**：游戏用 vID（`v<decimal>`，游戏名标识）键控，具体版本用 rID（`r<decimal>`，版本标识）键控，因此一条记录精确到某个游戏版本；同一 vID 的不同 rID 可携带不同分级。社区贡献者通过本仓 `.github/ISSUE_TEMPLATE/vndb-game-compatibility.yml` 提交结构化报告（vID、rID、分级、引擎、平台），维护者运行验证脚本把通过的条目落成 v2 记录并合并进数据仓。Bangumi 只用于游玩进度记录，不再参与兼容性库。

Manager 默认源由常量 `DEFAULT_COMPATIBILITY_SOURCE_URL` 给出，可经设置覆盖。拉取复用 metadata network-consent gate（启用 VNDB 网络访问后才允许），只接受 HTTPS、拒绝重定向，并以 SHA-256 content hash 做增量同步。相关 observability 事件为 `emu.compatibility.fetch`、`astra.emu.compatibility.cache`/`match`/`diagnostic` 与 `astra.emu.play.session_start`/`session_end`，字段只含 work_id、vID/rID、hash、status、diagnostic code 和计数，不含商业文本或本地路径。数据仓本身不在本计划交付范围，由用户单独创建维护。

## 日志命令

`astra-headless` 把 machine-readable protocol/report 写到 stdout，日志固定写到 stderr。通过 `ASTRA_LOG` 调整过滤器：

```bash
ASTRA_LOG=astra_headless=debug,astra_platform_headless=debug astra-headless serve --stdio --build-identity target/identity/astra-build-identity.json
```

需要落盘时由调用方显式重定向 stderr，protocol stdout 必须保持独立：

```bash
ASTRA_LOG=debug astra-headless serve --stdio --build-identity target/identity/astra-build-identity.json 2> target/logs/astra-headless.log
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
| `astra.headless_artifact_manifest.v2` | Headless submitted/rasterized 双流、PNG/WAV、render policy 和 renderer identity |
| `astra.headless_run_report.v2` | 平台无关 host、输入、双流产物与自动比较结果 |
| `astra.headless_review.v2` | 具名模型或人工的视觉/音频审查结果；不能覆盖自动失败 |
| `astra.headless_preflight_link.v2` | Headless E2 与真实平台 run 的 identity 关联 |
| `astra.plugin_report.v1` | 插件加载、卸载和 provider |

Stage 2 的 `astra package validate` 已输出 `astra.release_report.v1`，覆盖 package integrity、section bounds/hash、cook/project artifact、provider policy、media fallback policy、scenario refs、platform eligibility 和 platform report。`desktop-release`/`web-release` 缺 `compiled.project` 或 platform report 时阻断；headless/dev profile 的 platform report 可 warning。FFmpeg fallback 是 optional feature；profile 必须把缺失 FFmpeg 写成 warning 或 blocking。Release Gate check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。

ONNX Runtime local AI 发布时，operator 需要把 ModelBundle 当作 package 资产处理。模型、tokenizer、reduced runtime、Web runtime adapter 和 custom op sidecar 必须通过 cook/package 写入 Asset VFS section，并按 profile 绑定目标平台。Release Gate 校验 `ai.model_bundle`、`ai.model_bundle_vfs_mount`、`ai.onnx_runtime_pack`、`ai.onnx_execution_provider` 和 `ai.generated_artifact_save`；Windows、Linux、macOS/iOS、Android、Web 分别要求 `DirectML`、`OpenVINO`、`CoreML`、`QNN`、`WebNN` 主 EP 的真实目标运行证据。CPU fallback、release 阶段联网拉取 runtime、loose sidecar 或模型 payload 路径泄露都是 blocking diagnostic。
正式 Migration 8 evidence 使用 `python Tools/run_platform_host_acceptance.py ...` 汇总。脚本拒绝 dirty worktree，重新执行 Windows/Chrome host 测试，并校验两端 capability、conformance 与 Player report 的 package/profile/build/session continuity；输出 manifest 只包含 commit、hash、provider、check count、状态和 diagnostic，不包含输入文件路径。

## Android 构建与验收

Android 构建入口是 `python Tools/build_android.py`。operator 必须先安装并接受组织认可的 Android SDK license，准备 JDK 17、API 36、Build Tools 36.0.0、NDK 30.0.15729638 和 `cargo-ndk`。脚本使用独立 `CARGO_TARGET_DIR`，要求显式 `.astrapkg`、application id、target 和 output，不搜索共享 `target`，生成 debug APK、unsigned release APK/AAB 与 `astra.android_bundle_manifest.v1`。manifest 会记录实际 JDK 版本，以及 JDK runtime、Build Tools、NDK Clang 和 Gradle wrapper 的 hash；任一工具身份漂移都会改变 build fingerprint。release 签名仅通过 ignored 的外部 properties/keystore 注入。

```bash
python Tools/build_android.py --package Build/Game.astrapkg --target nativevn-game --application-id com.example.game --output Build/Android --with-emulator-abi
```

配置或 cross-build 通过只算 E1/E2。正式 E3 还必须在 API 28/36 emulator 与 arm64 Vulkan 真机上执行安装、启动、输入、TalkBack、MediaCodec、AAudio/focus、旋转/insets、SAF、save/recreate 和 zero-leak，并把同一 package/profile/build/session/input 的 host、Player、frame、audio、route 与人工 review 报告送入 release validator。OpenSL ES 只能用于显式 compatibility profile，实际报告为 OpenSL ES 时不得声称 AAudio。

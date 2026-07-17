# Runtime / Platform Operator Guide

Operator 负责构建、打包、平台适配、Release Gate、crash bundle 和 AstraEMU local case report。

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

文件与 stdio 使用同一双向 JSONL 协议。Headless profile v2 默认 `render_policy: checkpoints`：所有 scene 都校验并进入 submitted hash，仅首帧、具名 checkpoint 和末帧产生 RGBA；逐帧视觉验证显式使用 `all`。`max_submitted_frames` 与 `max_rasterized_frames` 任一超限都会阻断。stdout 只输出协议或 report，日志只写 stderr。

GPU job 必须先把 profile renderer 绑定为 `wgpu_offscreen`，再给 `run` 或 `serve` 传 `--gpu`；CPU profile 与 flag 混用会阻断。workspace test job 通过 `ASTRA_HEADLESS_GPU=1 cargo test ...` 让 `HeadlessTestContext` 生成 GPU profile 并启动 `serve --gpu`。该模式固定 Windows/DX12、Linux/Vulkan、macOS/Metal；Windows DX12 使用 build-locked static DXC，不读取 PATH 中的动态 DXC。所有平台都要求 hardware adapter，不回退软件 adapter或其他 backend。可用 `astra-headless benchmark-render --build-identity ... --output ... --gpu` 生成固定 1920x1080、60 帧预热、600 帧测量的性能报告。

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

## AstraEMU family 发布签名

桌面发布包把 FVP 动态库与 `manifest.json` 放在 Manager 可执行文件旁的 `families/fvp/`。构建 Manager 时必须通过受控构建环境提供 `ASTRA_EMU_FAMILY_SIGNER_ID` 和对应的 `ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX`；缺失 trust root 时产品启动会阻断。签名私钥只从短生命周期环境变量读取，不写入仓库、manifest、日志或报告。descriptor 必须取自同一 target/profile 的 FVP build-script 输出；`native-sign` 会先加载成品动态库并逐字段比对 descriptor，再生成唯一 manifest authority：

```bash
ASTRA_EMU_FAMILY_SIGNING_KEY_HEX="${RELEASE_SIGNING_KEY}" \
ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX="${RELEASE_PUBLIC_KEY}" \
cargo run -p astra-emu-family-package -- native-sign \
  --binary "target/release/${ASTRA_EMU_FVP_DYLIB}" \
  --descriptor "${FVP_BUILD_DESCRIPTOR}" \
  --output Build/Fvp/manifest.json \
  --target "${RUST_TARGET}" \
  --signer-identity "${ASTRA_EMU_FAMILY_SIGNER_ID}"
```

工具会校验 PE/ELF/Mach-O target 与 architecture、ABI root module、family/plugin/provider/engine/rustc/feature/ABI identity和 binary hash，再签名 canonical postcard identity。私钥对应的 public key 必须与 Manager trust root 一致；输出已存在、输入越界、descriptor 漂移、跨 target binary 或密钥不匹配都会阻断，不覆盖旧文件。旧 `sign` 只供受控迁移验证，不应作为新发布包的 identity authority。正式包仍需由平台 packaging gate 绑定 installer code-sign identity；family manifest 只能形成局部证据。

桌面分发统一由 `python Tools/build_astraemu_desktop.py --output Build/AstraEMU` 构建。该入口只接受本机 target，使用同一 release target root 构建 Manager、CLI 和 FVP，先从成品 ABI root module 校验 build-script descriptor，再生成签名 manifest，把两个 Program、动态库、manifest 和第三方 notices 原子提交到分发目录。`astra.emu.desktop_package_evidence.v1` 只记录相对文件名、hash、target、build identity 和 signer identity。`--development-ephemeral-signer` 仅用于本机 E3 调试，私钥只存在于当前进程且不会写盘；它生成的 development 包不得作为正式签名证据。

同一分发目录包含 `astra-emu-cli`。`run` 用于不受 Manager/overlay 影响的原生视觉验收：它会校验显式 family、授权目录、唯一 case、签名 manifest 和动态库 identity，随后直接创建 `AstraEmuRuntimeProvider` session 与 Windows platform host。窗口只显示 family 输出的 legacy 舞台，键盘、鼠标、触摸和手柄事件按舞台宽高比映射回 runtime。该模式默认静音，避免音频设备配置影响纯视觉对照；需要同时检查原生音频时显式传入 `--enable-audio`。

```bash
astra-emu-cli run --engine fvp --game-dir ./Games/Example --entry Game.hcb
```

`run` 不启动 Slint，也不读取 Manager Library、translation、patch 或 FilterGraph 配置。关闭窗口会依次 shutdown session、surface、window 和 platform host。Linux、macOS、iOS、Android 与 Web 尚未接入该原生 CLI host 时必须返回稳定的 `PLATFORM_NOT_IMPLEMENTED`，不能回退到 Manager 或 Headless 冒充原生验收。

自动化入口直接复用 `AstraEmuRuntimeProvider`、`RuntimeWorld` 和 `astra-platform-headless`，不启动 Slint，也不提供产品语义快捷命令。输入必须是有序、连续且以 `Shutdown` 结束的 `astra.user_input_sequence.v1` JSONL；只接受键盘、鼠标、触摸、手柄、IME、固定 tick 推进、物理观察等待和 checkpoint。输出目录包含真实 PNG/WAV artifact manifest 与 `astra.emu.headless_run_report.v1`，报告只保留 identity/hash/count/diagnostic：

```bash
astra-emu-cli headless \
  --engine fvp \
  --game-dir ./Games/Example \
  --entry Game.hcb \
  --input ./Automation/example-input.jsonl \
  --artifacts ./Build/AstraEMU-Evidence
```

`--verify-snapshot` 会在首个 checkpoint 执行同 session save/restore round-trip；输入没有 checkpoint 时会阻断。FVP snapshot 对每张 live graph 独立压缩精确 RGBA，并校验 decoded length 与像素 hash；脚本 texture alias 不会被当作可重开的本地路径。默认 `--artifact-retention checkpoints` 只落盘具名 checkpoint PNG，但全部提交帧仍进入 frame-stream hash；逐帧图像对照需要显式使用 `--artifact-retention all`。正式本地资源审计另加 `--audit-all-resources`，它在 gameplay run 后按 4 MiB range 流式读取全部可见资源，报告只保留资源/range 数、总字节数、最大 range 和 manifest hash，不写资源名或本地路径。Headless 结果只形成 E2，不能替代真实 Windows 窗口、GPU、音频设备和输入消费 E3。

Android 统一由 `python Tools/build_astraemu_android.py --abi arm64-v8a --abi x86_64` 构建。入口要求 API 36、NDK r28 以上、两种 Rust target、APK signer digest、family signer/trust root 和 Android keystore；它会检查 16 KiB ELF LOAD alignment，把每种 ABI 的动态库及双 manifest 写进 APK，执行 `apksigner verify`，最后输出不含 secret 和本地路径的 `astra.emu.android_package_evidence.v1`。缺 SDK license、签名身份或任一 ABI 都是 blocking，不能以手工复制 `.so` 代替。

iOS 工程由 `Emulator/Platforms/iOS/project.yml` 生成。Xcode build phase 调用 `build_for_ios_with_cargo.bash`，分别构建 device/simulator FVP archive，使用 `static-sign` 绑定 Mach-O archive architecture、descriptor、signer 和 trust root，再把同一 registration contract 静态链接进 Manager。device archive 必须由 Xcode 的有效 signing identity 签名；simulator 构建不能外推成真机 E3。

FVP 的固定行为基线是 rfvp `0.5.0` commit `3b5ea6c96a925c12f95aef8554905e8fecbc77c3`。`python Tools/verify_fvp_parity.py --reference .tmp/rfvp-reference` 只在本地受控环境运行：工具校验 reference revision，在临时 detached worktree 中执行 observer trace，并输出 `astra.frame_parity_report.v1`。CI 不联网拉取 RFVP。synthetic trace 只覆盖 parser/VM/Variant/context；实际游戏还要逐帧比较 semantic/RGBA/video PTS，并按固定音频容差检查 PCM。首差异保留前 30 帧和后 60 帧，任何自动比较失败都不能由人工审查覆盖。

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
| `astra.emu.local_case_report.v1` | AstraEMU FVP 和后续 family；只允许 alias/hash/offset/size 与稳定 diagnostic，禁止绝对路径和商业 payload |

Stage 2 的 `astra package validate` 已输出 `astra.release_report.v1`，覆盖 package integrity、section bounds/hash、cook/project artifact、provider policy、media fallback policy、scenario refs、platform eligibility 和 platform report。`desktop-release`/`web-release` 缺 `compiled.project` 或 platform report 时阻断；headless/dev profile 的 platform report 可 warning。FFmpeg fallback 是 optional feature；profile 必须把缺失 FFmpeg 写成 warning 或 blocking。Release Gate check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。

ONNX Runtime local AI 发布时，operator 需要把 ModelBundle 当作 package 资产处理。模型、tokenizer、reduced runtime、Web runtime adapter 和 custom op sidecar 必须通过 cook/package 写入 Asset VFS section，并按 profile 绑定目标平台。Release Gate 校验 `ai.model_bundle`、`ai.model_bundle_vfs_mount`、`ai.onnx_runtime_pack`、`ai.onnx_execution_provider` 和 `ai.generated_artifact_save`；Windows、Linux、macOS/iOS、Android、Web 分别要求 `DirectML`、`OpenVINO`、`CoreML`、`QNN`、`WebNN` 主 EP 的真实目标运行证据。CPU fallback、release 阶段联网拉取 runtime、loose sidecar 或模型 payload 路径泄露都是 blocking diagnostic。
正式 Migration 8 evidence 使用 `python Tools/run_platform_host_acceptance.py ...` 汇总。脚本拒绝 dirty worktree，重新执行 Windows/Chrome host 测试，并校验两端 capability、conformance 与 Player report 的 package/profile/build/session continuity；输出 manifest 只包含 commit、hash、provider、check count、状态和 diagnostic，不包含输入文件路径。

## Android 构建与验收

Android 构建入口是 `python Tools/build_android.py`。operator 必须先安装并接受组织认可的 Android SDK license，准备 JDK 17、API 36、Build Tools 36.0.0、NDK 30.0.15729638 和 `cargo-ndk`。脚本使用独立 `CARGO_TARGET_DIR`，要求显式 `.astrapkg`、application id、target 和 output，不搜索共享 `target`，生成 debug APK、unsigned release APK/AAB 与 `astra.android_bundle_manifest.v1`。manifest 会记录实际 JDK 版本，以及 JDK runtime、Build Tools、NDK Clang 和 Gradle wrapper 的 hash；任一工具身份漂移都会改变 build fingerprint。release 签名仅通过 ignored 的外部 properties/keystore 注入。

```bash
python Tools/build_android.py --package Build/Game.astrapkg --target nativevn-game --application-id com.example.game --output Build/Android --with-emulator-abi
```

配置或 cross-build 通过只算 E1/E2。正式 E3 还必须在 API 28/36 emulator 与 arm64 Vulkan 真机上执行安装、启动、输入、TalkBack、MediaCodec、AAudio/focus、旋转/insets、SAF、save/recreate 和 zero-leak，并把同一 package/profile/build/session/input 的 host、Player、frame、audio、route 与人工 review 报告送入 release validator。OpenSL ES 只能用于显式 compatibility profile，实际报告为 OpenSL ES 时不得声称 AAudio。

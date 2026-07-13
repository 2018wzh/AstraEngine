# Implementation Plan Status

本页只记录当前代码完成度和下一步实施顺序。设计规格放在 `Docs/product`、`Docs/contracts`、`Docs/modules` 和 `Docs/implementation`；本页不把已规划内容写成已实现能力。

## 状态规则

| Status | 含义 | 允许标记条件 |
| --- | --- | --- |
| `DONE` | 已实现并有可运行证据 | 目标路径存在，关联测试通过，状态页写明证据命令 |
| `IN_PROGRESS` | 正在实现 | 已有代码落地，但关联 gate 未全部通过 |
| `REOPENED_SPEC` | 已有阶段口径因新契约重开 | 既有证据继续保留，但新增 contract、gate 或迁移边界尚未落地 |
| `SPEC_READY` | 设计可开工 | 文档、schema、gate 和测试映射已写清，代码未落地 |
| `RESEARCH_READY` | 研究资料可用于实现 | 仅限 AstraEMU family 研究和 probe 工具，不代表 runtime 已实现 |
| `NOT_STARTED` | 尚未开工 | 目标 crate、scenario 或 gate 仍不存在 |

以后实现某个工作项时，同步改本页、对应 `stage-*.md`、[stage-test-matrix.md](stages/stage-test-matrix.md) 和 [coverage-matrix.md](coverage-matrix.md)。没有测试或 release report 证据，不标 `DONE`。

## 当前代码快照

| Area | Code status | Evidence |
| --- | --- | --- |
| Stage 1 EngineCore | `DONE` | `python Tools/run_cargo_isolated.py test --workspace` 通过；checkout-bound identity绑定 commit/dirty state、workspace manifests、Cargo.lock、toolchain与features，动态 fixture build/load使用同一 target root；Runtime snapshot保存 stable id generator、完整 EventQueue/AwaitQueue/delayed queue、typed component、mutation/effect trace；flat FSM支持 run-to-quiescence和事务回滚；provider-free replay校验 input/await/provider output与逐 tick hash |
| Stage 2 Media + Package | `IN_PROGRESS` | 生产完备度审查已重开本 Stage。VFS context/conflict authority、container bounds/hash/AAD/duplicate 校验、`astra.schema_registry.v2`、required-section schema authority、`astra.scenario_refs.v2` path/section/hash identity 和真实产品 package constructor 已落地。仍缺 Cook 依赖图、内容寻址持久缓存、bounded concurrency、取消和原子批次提交，以及非 Web renderer/media 设备恢复、队列、资源释放和性能门禁。Migration 11 另行重开完整 Headless Platform 口径：当前没有 `astra-platform-headless`、统一 JSONL 物理输入、真实 PNG/WAV artifact、全 Runtime test 收束、模型审查或真实平台 preflight link；九个 `S2-HEADLESS-*` 均为 `SPEC_READY`。这些缺口关闭前不能恢复 `DONE`；Linux/macOS/iOS/Android 仍属于 Stage 6 |
| Stage 3 AstraVN | `IN_PROGRESS` | Migration 6 frontend 与 Migration 9 shared 1–3 focused implementation 已落地；NativeVN 已收敛为两路线技术样例并移除 SAPI 配音。`S3-SCRIPT-01/02`、presentation、system UI、sample、Windows/Web Player 均等待同一 `.astrapkg` 的 formal native-input evidence。顶层仍由 `S3-TSUI-INTERNAL-DEMO-01`、`S3-TSUI-GATE-01` 和 `S3-FLAGSHIP-DEMO-01` 阻断 |
| Stage 4 Editor + AI/MCP | `REOPENED_SPEC` | Editor workflow、runtime-provider-aware shell、Plugin Manager、AI provider profile、ONNX ModelBundle、Runtime Director、memory、MCP context 和 AI/MCP gate 已写入文档；`Editor/Source` 和 `Engine/Plugins/Providers/astra-ai-onnx` 尚不存在。Stage 4 因 VFS/GameRuntime contract 重开，Project Wizard、PIE、Debugger 和 Release Gate 必须读取 `RuntimeEditorMetadata`，ONNX ModelBundle、Context Pack、generated artifact 和 MCP package access 需要改为统一 VFS mount evidence |
| Stage 5 AstraEMU | `REOPENED_SPEC` | `Docs/contracts/astraemu-ipc.md`、`Docs/implementation/astraemu-legacy-runtime-framework.md`、`Docs/implementation/emulator-core-state-machine.md` 和 `Docs/emu` 已写清 `AstraEmuRuntimeProvider`、`LegacyRuntimeProvider` facade、VM 状态机映射、auto probe、Trusted Luau、文本翻译、FilterGraph preset 和 legacy pack VFS mount；`AstraEMU/Source` 尚不存在，不能把 full-family runtime 写成已实现 |
| Stage 6 Platform Completion | `SPEC_READY` | Linux、macOS、iOS 和 Android 的真实 host smoke、launcher、platform decode、save/resume 和 release evidence 已移到 [stage-6-platform-completion](stages/stage-6-platform-completion.md)；当前只保留 capability crate 和 planned gate |
| Stage 7 AstraRPG | `SPEC_READY` | [AstraRPG contract](../contracts/rpg-trpg.md)、[runtime blueprint](../implementation/astra-rpg-runtime.md)、[Stage 7](stages/stage-7-astra-rpg.md)、coverage matrix 和 stage-test matrix 已写清 `AstraRpgRuntimeProvider`、RPG core、AI Town、`rpg.trpg` profile、CP2020 local-private adapter 和 release gate 目标；`Engine/Source/Modules/AstraRPG/` 尚不存在，不能写成已实现 |
| Stage 8 AstraRPG Server/Client Protocol | `SPEC_READY` | [Stage 8](stages/stage-8-astra-rpg-network.md) 已写清 `rpg.net.*` protocol、server/client session、seat sync、transcript sync 和 network replay gate；无 protocol crate、server/client crate 或 release gate 实现，且不阻塞 Stage 7 |

Stage 3 补充证据：TsuiNoSora 本地 helper 已生成 `tsuinosora.projectorrays_reader_report.v1`、`tsuinosora.projectorrays_full_dump_report.v1`、脱敏 `tsuinosora.script_source_map.v1` 和 `tsuinosora.script_source_map_report.v1`，ProjectorRays 只作为 ignored config 指向的本地工具来源，report 不写脚本文本、payload、bytecode、原始 dump 或本地路径；在 `demo-slice` 中，ProjectorRays pass report 只满足 Director reader-required preflight，不会消除 payload、路径、hash、resource 断裂或 route/source-map 缺失 diagnostic。Full-resource classic playable bundle 现在要求 `projectorrays_full_dump_report.resource_coverage` 覆盖每个 ProjectorRays binary chunk；该 report 已输出 `chunk_fourcc_counts`、`conversion_plan` 和从 ignored `tsuinosora.projectorrays_converted_resources.v1` sidecar 校验得到的 `converted_resources`，用于按 `BITD`、`STXT`、`snd `、`ediM`、`XMED`、`CASt`、`Lscr` 等类别推进真实转换，并阻断 hash-only、route-only 或 raw chunk copy。`projectorrays-convert-resources` 已能把 JSON-backed Director metadata chunk 转成脱敏 `native-assets/projectorrays/...` metadata asset，把 `STXT` 解码为私有 UTF-8 text asset，把可证明通过 cast member、same-scope `scriptNumber`、`CastScript`/`ParentScript` source 或 malformed JSON numeric recovery 绑定到 `.ls/.lasm` 的非空 `Lscr` 转成 private script asset，把 empty ProjectorRays `Lscr` metadata 转成脱敏 no-op script metadata，通过 `KEY_`/bitmap `CASt` metadata 把 1/16/32bpp `BITD` 转成 RGBA PNG，并支持用私有 `tsuinosora.projectorrays_palette_sidecar.v1` 把 palette 证据匹配的 8bpp `BITD` 转成 PNG；它还会把 empty `snd `、zero `cupt`、`SCRF`、`Cinf`、`VWFI`、`Sord`、`Fmap`、`VWLB`、`FCOL`、`FXmp`、`VERS`、`XTRl`、`VWSC` 和 `XMED` 转成脱敏结构 metadata，把 KEY-bound `sndH`/`sndS` Moa PCM 转成 WAV audio asset，并把 KEY-bound `ediM` `MACRZ` 媒体解析为经 frame 边界校验的 MP3 audio asset，report 只记录 stream offset/byte size/hash、frame count、sample rate、bitrate、channel count、marker/GUID hash 和 diagnostic，不写媒体 payload；当前私有 dump 覆盖 2527 个 binary chunk，已转换 2527 个 chunk，conversion diagnostics 为 0。Asset analysis pass 后会真实写入 `native-assets/`，生成 asset sidecar，把 source/native 相对路径、classification、source hash、converted hash 和 byte size 汇入 `tsuinosora.conversion_report.v1`；`astra-cli cook` 会读取 `nativevn.asset_roots`，经 `astra-cook` 写入 cooked asset package section，并在 `asset.vfs_manifest.entries` 中登记 `package:/native-assets/...` URI、section、hash 和 byte size，同时在 `asset.catalog.assets` 中登记 asset id、media kind 和 profile；cook 还会把 bundle 需要的 scenario refs 写成 package sections，standalone bundle 从同一个 `.astrapkg` 读取 scenario refs，不依赖源码目录中的外部 scenario 文件。NativeVN package input 已把 route graph/source map 的 sanitized choice id 保留到 `.astra` option key 和 scenario `player_input choose`，多 choice route 不再压缩成单个 synthetic choice；ProjectorRays `GO[...]` script identity 可派生脱敏 route/source-map evidence，`stage3-gate` 派生的 route-bound cast member 会通过 source/native hash 写入 `mount_assets`，并在没有显式 routes 入参的 `local-gate` 中继续进入 conversion report 和 patch/windows scenario refs。`demo-slice --config` 已能从私有 root config 调用 `local-gate`，生成 `tsuinosora.demo_slice_report.v1`、可 cook/package 的 NativeVN project、脱敏 package sections、Windows/Web standalone bundle 输入和 player route scenarios；`internal-demo-bundle` 当前已从同一 package 产出 Windows/Web bundle manifest，并会用 `visual_capture.capture_automation` 的 Windows `SendInput`/GDI backend 执行原版/Demo 同 checkpoint 截图采集，私有原版入口已能通过 launch environment 以当前用户启动并采集窗口截图；Windows Demo bundle 现在按 project display 原始分辨率打开 live window，`astra-player` 能启动 bundle、聚焦窗口、用 `SendInput` 生成 `astra.player_input_transcript.v1`、绑定视觉 comparison report hash、从 package 内 WAV 资源生成 audio meter，并输出 `astra.player_automation_report.v1`。验收仍因 live input 后 Demo 可见区域 hash 未变化、automation transcript 未覆盖 28 条 full classic route、缺 required manual signoff 而 blocking。静态 PNG 与视觉 review 不能绕过 `execution_status: pass`。`demo-config-template` 和 `Examples/TsuiNoSora/Docs/demo.config.template.json` 提供 repo-relative `.local` scaffold，不写本地绝对路径。`astra-player-core`、`astra-player` 和 `astra-release` 已落地 `astra.player_automation_script.v1`、`astra.player_input_transcript.v1`、`astra.player_automation_report.v1` 与 `player.full_playable` evidence 校验；默认 package validate 不会把 route report 当成 full playable，只有显式传入匹配 package hash/profile/target 的 live automation report 才能通过该 check。Stage 3 仍不能标 `DONE`。

## Stage 1 完成项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S1-BOOT-01` | `DONE` | workspace、toolchain、CI 配置存在 |
| `S1-BUILD-IDENTITY-01` | `DONE` | `python -m unittest Tools.tests.test_run_cargo_isolated`、`python Tools/run_cargo_isolated.py test -p astra-plugin --test artifact_path`、`python Tools/run_cargo_isolated.py test -p astra-cli --test logging` 和隔离 workspace tests；`astra.build_identity.v1` 记录 checkout/manifest/lock/toolchain/feature identity 与相对 artifact hash |
| `S1-CORE-01` | `DONE` | `cargo test -p astra-core core_types` |
| `S1-RUNTIME-01` | `DONE` | `cargo test -p astra-runtime world_actor` 和 `cargo test -p astra-runtime trigger_event`；typed component payload/hash、component mutation record 和 trigger event 已覆盖 |
| `S1-RUNTIME-02` | `DONE` | `cargo test -p astra-runtime state_machine_tick`；run-to-quiescence、terminal、cycle/microstep blocker 和整机事务回滚已覆盖 |
| `S1-RUNTIME-04` | `DONE` | `cargo test -p astra-runtime state_machine_tick`、`cargo test -p astra-runtime delayed_event` 和 `cargo test -p astra-runtime trigger_event`；ActionRegistry conflict/schema descriptor、typed mutation、serialized effect 和 delayed event 已覆盖 |
| `S1-RUNTIME-03` | `DONE` | `cargo test -p astra-runtime await_token`；RecordedResult/DeterministicTimeout policy、乱序结果、非法 completion 和 timeout materialization 已覆盖 |
| `S1-SAVE-01` | `DONE` | `cargo test -p astra-runtime save_replay`；stable id continuation、完整 EventQueue pending/trace/sequence、provider-free replay output/hash 和逐 tick checkpoint 已覆盖 |
| `S1-DYLIB-01` | `DONE` | `cargo test -p astra-engine dylib_facade` |
| `S1-PLUGIN-01` | `DONE` | `cargo test -p astra-plugin descriptor_gate` and `cargo test -p astra-plugin load_unload` |
| `S1-PLUGIN-02` | `DONE` | `cargo test -p astra-plugin ffi_action_provider` |
| `S1-PLUGIN-03` | `DONE` | `cargo test -p astra-plugin extension_registry` |
| `S1-PROP-01` | `DONE` | `cargo test -p astra-property --test property_metadata` and `cargo test -p astra-property --test expand_smoke` |
| `S1-TEST-01` | `DONE` | `cargo test -p astra-test native_smoke` |
| `S1-OBS-01` | `DONE` | `cargo test -p astra-cli --test logging` |
| `S1-TARGET-01` | `DONE` | `cargo test -p astra-target` and `cargo test -p astra-cli --test target_platform` |

## 跨 Stage 可观测性

| Work ID | Status | Evidence |
| --- | --- | --- |
| `OBS-CORE-01` | `DONE` | `cargo test -p astra-observability`、`cargo test -p astra-cli --test logging`、`cargo test -p astra-player` 和 `cargo test --workspace` 通过；覆盖 `astra.log_event.v1`、五级语义、reload、span/session、限额 file/ring、queue saturation、critical mirror、fatal bundle、隐私清洗与日志开关前后 deterministic hash 一致 |
| `OBS-COVERAGE-01` | `DONE` | `python Tools/check_observability.py` 通过，当前 41 个 workspace crate 均为 `instrumented` 或有明确 `not_applicable` 原因；`python Tools/check_docs.py`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo test --workspace` 通过 |
| `OBS-CRASH-WIN-01` | `DONE` | `cargo test -p astra-crash-reporter -- --test-threads=1` 和 `cargo test -p astra-cli --test target_platform nativevn_sample_builds_windows_and_web_bundles_and_runs_player_routes -- --exact` 通过；真实 `MDMP`、进程外 panic/SEH request、hash/size/manifest、v2 bundle role/hash/self-test、required 启动握手与 tamper blocker 已验证 |

## Stage 2 平台修复项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S2-PLATFORM-01` | `IN_PROGRESS` | Migration 8 已落地 async typed-handle contract、`astra-platform-general`、capability v2 与 conformance schema；Windows/Web 同 commit 真实验收尚未闭合，不能恢复 `DONE` |
| `S2-HEADLESS-CONTRACT-01` | `SPEC_READY` | Migration 11 规划 `HostKind`、`HeadlessHostProfile` 与 `HostLaunchProfile`；`PlatformId` 和发布 profile v2 保持六平台，代码尚未实现 |
| `S2-HEADLESS-HOST-01` | `SPEC_READY` | planned `publish = false` `astra-platform-headless` 尚不存在；surface/audio/decode/save/package/input 与 zero-leak lifecycle 未统一 |
| `S2-HEADLESS-MEDIA-01` | `SPEC_READY` | planned Media-owned reference providers 与真实 PNG/PCM WAV 输出尚未接入；现有 CPU frame/meter 不能代表完整后端 |
| `S2-HEADLESS-INPUT-01` | `SPEC_READY` | `astra.user_input_sequence.v1` 与双向 JSONL `astra.headless_protocol.v1` 只有文档，尚无 Rust schema 或 runner |
| `S2-HEADLESS-ARTIFACT-01` | `SPEC_READY` | PNG/WAV retention、限额、artifact manifest 和 run report 只有文档，尚无产物证据 |
| `S2-HEADLESS-CLI-01` | `SPEC_READY` | planned `astra-headless run` / `serve --stdio` binary 尚不存在；旧 `--headless` 仍是当前实现入口 |
| `S2-HEADLESS-TEST-MIGRATION-01` | `SPEC_READY` | 全部 Runtime test 启动 `HeadlessTestContext` 的收束尚未实施，现有直连 provider/mock/scenario 路径继续计为迁移输入 |
| `S2-HEADLESS-REVIEW-01` | `SPEC_READY` | 全帧/全音频自动比较、模型 checkpoint 审查、人工容差批准与 `astra.headless_review.v1` 尚未实现 |
| `S2-HEADLESS-PREFLIGHT-01` | `SPEC_READY` | 同 build/package/input 的 Headless→真实平台强制 preflight link 尚未实现，Headless 结果仍不能作为 E3 |
| `S2-WINDOWS-HOST-01` | `IN_PROGRESS` | `cargo test -p astra-platform-windows --features platform-test-driver` 已覆盖 real window、hardware wgpu present/readback、WASAPI callback、WMF、Saved Games、package range 与 SendInput→host event；正式 conformance/Player continuity 尚未闭合 |
| `S2-WINDOWS-WMF-01` | `DONE` | `cargo test -p astra-media decode_provider`；public media manifest 校验固定 sha256，WMF `decode.wmf.audio` 输出 MP3 bounded PCM CPU buffer，`decode.wmf.video_first_frame` 输出 MP4 BGRA 首帧，invalid video 返回 `ASTRA_WMF_DECODE` |
| `S2-WINDOWS-GATE-01` | `DONE` | `cargo test -p astra-release release_report` and `cargo test -p astra-cli --test target_platform`；缺 required Windows smoke 会阻断 release check |
| `S2-PLUGIN-GATE-01` | `DONE` | `cargo test -p astra-package package_roundtrip` and `cargo test -p astra-release release_report`；package 写入 `plugin.extension_registry` 和 `plugin.dependency_graph`，release gate 阻断 unresolved conflict、missing binding 和 unresolved dependency |
| `S2-RUNTIME-FSM-01` | `DONE` | `cargo test -p astra-runtime --test state_machine_tick`；flat FSM validation、terminal/completed、priority conflict diagnostic 和 Always tick trigger 已落地 |
| `S2-RUNTIME-AWAIT-01` | `DONE` | `cargo test -p astra-runtime --test await_token`；Await timeout materialization、unknown/duplicate result diagnostic 和 pending token serialization 已落地 |
| `S2-SCENARIO-GATE-01` | `DONE` | `cargo test -p astra-test --test native_smoke`；unknown VN action/assertion blocked、declared package missing blocked、Stage 1 native smoke pass |
| `S2-WEB-HOST-01` | `IN_PROGRESS` | Chrome canvas/WebGPU、WebCodecs、OPFS、File/fetch、typed lifecycle/input 与 AudioWorklet queue 已落地；policy/provider DTO 已分别与 native mlua executor、`abi_stable`/`libloading` dynamic loader 分层，`cargo check --target wasm32-unknown-unknown` 与 `wasm-pack build --target web` 已通过。用户手势 audio meter、device/context loss、真实 CDP 输入和正式 Player evidence 尚未闭合 |
| `S2-VFS-01` | `DONE` | `cargo test -p astra-asset vfs_uri`、`cargo test -p astra-asset vfs_overlayfs`、`cargo test -p astra-package package_vfs_mount`、`cargo test -p astra-plugin vfs_provider_registry`、`cargo test -p astra-release vfs_mount_gate` 和 `cargo test -p astra-cli --test target_platform tsuinosora_synthetic_gate_runs_internal_and_patch_player_routes` 覆盖 `provider:/path/file` URI、prefix registry、package-backed VFS manifest、独立 `asset.catalog`、旧 `asset.registry` blocking、多 `vfs_provider` 同 slot、overlay whiteout、local root 不序列化和 TsuiNoSora package asset VFS continuity；legacy pack reader 实现仍留 Stage 5 |

## Stage 3 进行中项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S3-MODULE-LAYOUT-01` | `DONE` | `cargo metadata --no-deps`、`cargo test -p astra-vn --test vn_dylib_facade` 和 `cargo test -p astra-test --test vn_scenario` 通过；`astra-vn` 已迁到 `Engine/Source/Modules/AstraVN/astra-vn`，workspace/path dependency 指向新位置 |
| `S3-CRATE-SPLIT-01` | `DONE` | `cargo test -p astra-vn-script`、`cargo test -p astra-vn-core`、`cargo test -p astra-vn-policy`、`cargo test -p astra-vn-presentation`、`cargo test -p astra-vn-commands`、`cargo test -p astra-vn-system`、`cargo test -p astra-vn-save`、`cargo test -p astra-vn-package`、`cargo test -p astra-vn-plugin`、`cargo test -p astra-vn-editor` 和 `cargo test -p astra-vn-runtime-provider` 通过；功能 crate 不依赖 facade |
| `S3-DYLIB-01` | `DONE` | `cargo test -p astra-vn --test vn_dylib_facade` 和 `cargo test -p astra-vn-plugin --test vn_plugin_extensions` 通过；`astra-vn` 只保留 `rlib`/Rust ABI `dylib` facade 和 AstraVN 子 crate re-export |
| `S3-RUNTIME-PROVIDER-01` | `DONE` | `cargo test -p astra-plugin-abi runtime_provider_abi`、`cargo test -p astra-plugin runtime_provider_registry`、`cargo test -p astra-vn-runtime-provider --test game_runtime_provider`、`cargo test -p astra-vn-runtime-provider --test runtime_provider_ffi`、`cargo test -p astra-test --test vn_scenario`、`cargo test -p astra-release --test release_report runtime_provider` 和 `cargo test -p astra-cli --test target_platform nativevn_sample_cooks_packages_validates_and_runs_full_playthrough` 通过；FFI 覆盖 provider instance create/destroy、package-bound open、真实 step/save/restore/shutdown 和活动 session 销毁阻断；release report 记录 behavior state/event/presentation hash |
| `S3-SCRIPT-01` | `IN_PROGRESS` | Migration 6 focused evidence 已通过：`cargo test -p astra-vn-script --test frontend` 覆盖 logos/chumsky/rowan/text-size frontend、层级 CST、CST-backed Typed AST、unknown command binding、trivia round-trip 与 language-service navigation。状态等待 Stage 3 formal Windows/Web Player evidence |
| `S3-SCRIPT-02` | `IN_PROGRESS` | 固定 symbols/routes/variables/commands/system stories/compiled story pass、semantic hash、独立 source-map hash、formatter semantic guard 和 `ASTRA_VN_RECOOK_REQUIRED` 已有 focused tests；状态等待 Stage 3 formal Windows/Web Player evidence |
| `S3-CORE-01` | `DONE` | `cargo test -p astra-vn-core --test compiler_runtime`、`cargo test -p astra-vn-core --test await_gates`、`cargo test -p astra-vn-package --test commercial_baseline`、`cargo test -p astra-vn-system --test system_controls`、`cargo test -p astra-runtime --test trigger_event`、`cargo test -p astra-vn-runtime-provider --test game_runtime_provider` 和 `cargo test -p astra-test --test vn_scenario` 覆盖结构化 cursor、dialogue/choice/system wait、Runtime AwaitToken、call/system stack、audio/timeline/effect、mutation、route flag 和 `astra.vn.step` action trace |
| `S3-CORE-02` | `DONE` | `cargo test -p astra-vn-core --test compiler_runtime`、`cargo test -p astra-vn-system --test system_controls` 和 `cargo test -p astra-vn-save --test vn_save_container` 覆盖 rich `BacklogEntry`、read-state、voice replay、skip-read eligibility、`VnReplayUiState` replay UI snapshot、replay UI hash 和 save/load roundtrip |
| `S3-CORE-03` | `DONE` | `cargo test -p astra-vn-core --test compiler_runtime`、`cargo test -p astra-vn-save --test vn_save_container`、`cargo test -p astra-vn-runtime-provider`、`cargo test -p astra-player-vn --test native_vn_host_source` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 VN save/load、`vn.runtime_state`、`vn.policy_state`、`vn.runtime_world` 完整 `RuntimeSnapshot`、Player save integrity、await 恢复、backlog/read-state/voice replay/route flags/变量、Luau serializable snapshot、mutation/effect/event queue 和 replay hash |
| `S3-LUAU-01` | `DONE` | `cargo test -p astra-vn-policy --test luau_sandbox` 和 `cargo test -p astra-vn-policy --test luau_mutation` 覆盖 capability sandbox、真实 `PolicyQueryContext` text/asset/backlog/savepoint/layout backing、query result hash、interrupt/memory/output/snapshot-depth budget、`astra.var.set` authority bypass blocking、记录型 mutation/command/trace、rollback/replay 和不可序列化 payload blocking |
| `S3-LUAU-02` | `DONE` | `cargo test -p astra-vn-policy --test policy_bundle` 和 `cargo test -p astra-release --test release_report release_gate_` 覆盖官方 `astra.policy.standard` Luau source cache、真实 source hash/byte size、`vn.policy_bundle_manifest`、`vn.policy_bundle_source_cache`、required capabilities、package lock、缺 source cache blocking diagnostic、hash mismatch blocking diagnostic 和 sandbox 执行注册 command/trace/snapshot |
| `S3-PLUGIN-01` | `DONE` | `cargo test -p astra-vn-plugin --test vn_plugin_extensions` 和 `cargo test -p astra-release --test release_report release_gate_` 覆盖 Luau policy bundle provider、VN command provider、presentation command provider、Graph/Timeline metadata extension、release check provider 显式绑定、缺失 binding 和重复 binding 阻断 |
| `S3-PRESENT-01` | `IN_PROGRESS` | `SceneCommand` resource upload/release、sprite source rect、glyph run、clip/transform/camera/filter 与扩展 StageModel 已通过 CPU reference tests；共享 `NativeVnProductMediaHost` 已供 Windows/Web 执行 timeline completion、decode、持久 mixer、wait completion 和 cleanup，内部 audio owner 覆盖 user-activation resume、设备格式协商、bounded sinc resampling、受限声道映射、BGM loop、bus fade、pause/resume/stop、stable voice completion、queue backpressure/underflow、时长感知 drain 和显式 close。设备热切换恢复、Windows wgpu/WebGPU command stream 和 formal E3 evidence 尚未闭合 |
| `S3-ADVANCED-01` | `IN_PROGRESS` | AdvancedVN 技术命令已并入两路线 `Examples/NativeVN`，重复 `Examples/AdvancedVN` 已删除；NativeVN script check/cook 通过，仍等待 Windows/Web formal advanced presentation evidence |
| `S3-SYSTEM-01` | `IN_PROGRESS` | `SystemUiModel` 覆盖 title/message/choice/save/load/config/backlog/gallery/replay/voice replay/route chart/localization preview；Windows/Web Player 的 F5/F9 已接入平台 `BeginSave/WriteSave/CommitSave/ReadSave` transaction、写入或提交失败后显式 abort、provider restore 和恢复帧重新 present。真实渲染与 formal host evidence 尚未闭合 |
| `S3-EDIT-01` | `DONE` | `cargo test -p astra-vn-editor --test editor_metadata` 覆盖 Graph node、Timeline track、wait/fence metadata 到 command id/source map 的校验、缺 command blocking diagnostic 和 patch manifest 只引用同一 IR command id |
| `S3-SAMPLE-01` | `IN_PROGRESS` | NativeVN 已收敛为两路线技术验收样例，图片、OFL 字体、CC0 voice sample、BGM/SE/video 均有 sidecar、license、provenance、hash 与 byte size，script check/cook 通过；formal runner 未通过前保持进行中 |
| `S3-GAME-TARGET-01` | `IN_PROGRESS` | `cargo test -p astra-cli --test target_platform`、`cargo test -p astra-release --test release_report release_gate_` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 `nativevn-game` package gate、VN package section、scenario field、Windows/Web standalone bundle manifest 与 scenario refs。Web bundle 现在只接受经 `wasmparser` 校验的 wasm 与固定名称 glue，canonical loader/worklet 由 CLI 内嵌，构建经 staging 原子提交并阻断 route/DOM bypass marker。既有 route report slice 仍不证明 live player 由真实 CDP 输入完整可玩 |
| `S3-PLAYER-AUTOMATION-01` | `IN_PROGRESS` | `cargo test -p astra-player-core`、Player automation tests、`cargo test -p astra-player-vn --test native_vn_host_source --test product_audio_host` 和 `cargo test -p astra-player-web --test package_identity` 覆盖 transcript v2、scenario-derived physical input、Runtime route/terminal evidence、Windows/Web 原子 save/load、共享 media completion、persistent mixer、decode/audio cleanup，以及 versioned Web console evidence 的 package/provider/session/step/hash/route/choice/meter identity。direct route/DOM/JS callback bypass 继续阻断；多 route 独立 session、正式 Web CDP driver 和同 run screenshot/audio/route evidence 尚未闭合 |
| `S3-FLAGSHIP-DEMO-01` | `IN_PROGRESS` | 15–20 分钟、三终局、中英双语、中文全配音和正式原创资产进入 `Docs/migrations/nativevn-flagship-demo-migration.md`；本轮不实现，SAPI/TTS 产物不得进入公开样例 |
| `S3-TSUI-INTERNAL-DEMO-01` | `IN_PROGRESS` | `python Tools/TsuiNoSora/tests/test_asset_analysis.py`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k internal_demo_bundle`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k visual`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k projectorrays`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k projectorrays_full_dump`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k full_resource_conversion`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k nativevn_package_input`、`python Tools/check_docs.py`、`cargo test -p astra-cli --test target_platform tsuinosora_internal_demo_builds_asset_package_and_bundles`、`cargo test -p astra-player-core`、`cargo test -p astra-player --test windows_input_automation`、`cargo test -p astra-player --test web_input_automation` 和 `cargo test -p astra-release release_gate_accepts_player_full_playable_only_with_matching_live_report` 覆盖 `tsuinosora-internal-game` `classic` full-resource playable bundle pipeline shape：ProjectorRays 本地 dump adapter 脱敏、Director reader-required preflight 外部 reader evidence、`tsuinosora.projectorrays_full_dump_report.v1`、`tsuinosora.projectorrays_converted_resources.v1` sidecar 校验、JSON-backed metadata chunk converter、`STXT` text converter、`Lscr` cast-member/source-number/CastScript/ParentScript 映射和 malformed JSON numeric recovery、empty `Lscr` no-op metadata converter、`BITD` 1/16/32bpp PNG converter、8bpp `BITD` palette sidecar converter、KEY-bound `sndH`/`sndS` WAV converter、KEY-bound `ediM` `MACRZ` verified MP3 converter、score/metadata chunk 脱敏 converter、全量 binary chunk 转换覆盖 blocking、ProjectorRays `GO[...]` route identity 派生、ProjectorRays converted asset bridge、NativeVN `asset_roots`、asset sidecar、cooked asset package section、`asset.vfs_manifest`/`asset.catalog` package VFS evidence、scenario refs package section、同一 `.astrapkg` 派生 Windows/Web bundle manifest、`tsuinosora.visual_screenshot_capture_report.v1`、`tsuinosora.visual_comparison_report.v1`、`capture_automation` 自动截图 intent/execution 脱敏记录、`demo-config-template` repo-relative `.local` scaffold 和 `visual_capture` 配置，以及 `player.full_playable` live report 校验。当前私有运行已生成 full dump coverage，2527 个 ProjectorRays binary chunk 均有 converted evidence，`demo-slice` 可生成 28 条脱敏 route 的 NativeVN project/package input，`internal-demo-bundle` 已产出 Windows/Web bundle manifest，title checkpoint 视觉 comparison 通过，Windows live automation 已证明 28 次 `SendInput` 被 player host 消费并生成 audio meter；验收仍因真实输入后 Demo 可见区域 hash 未变化、full route coverage 缺失和 required manual signoff 而 blocking，不能标 `DONE`。`modern`、Patch-only、Runtime Patch/VFS 插件不属于本 milestone |
| `S3-TSUI-GATE-01` | `IN_PROGRESS` | `python Tools/TsuiNoSora/tests/test_asset_analysis.py` 覆盖脱敏 inventory、format probe、edition fingerprint、`tsuinosora.extract_report.v1` direct-readable sidecar 复制、Director `imap`/`mmap` resource map preflight 和 `free_resource_count`、受限 `XFIR` RIFF/RIFX exact wrapper reader、opaque/compressed `XFIR` 与尾随未验证 bytes reader-required 阻断、Director `KEY*`/`CAS*` cast map preflight、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight、`Lnam` entry count/table hash、从 Director cast map 与 child resource id/FourCC/extracted payload hash 派生 `tsuinosora.cast_source_map_report.v1`、受限 RIFF/RIFX chunk 表读取、embedded image/audio/movie/script text/metadata JSON payload 抽取、手写 `tsuinosora.cast_map.v1` 的 member/source/container entry/hash 映射、cast sidecar source hash mismatch 阻断、`tsuinosora.script_source_map_report.v1` route marker/source line 派生、可读或短 binary-header wrapped mapped `Lscr` 自动生成 `director_lingo_source_map.json`、`tsuinosora.script_source_map.v1` 脱敏 reader sidecar 派生、reader id/hash/output contract evidence、payload/path/hash/symbol blocking、route graph payload/unsafe symbol blocking、reader sidecar declared source hash mismatch blocking、route line out-of-range blocking、sidecar route source/hash mismatch blocking、合规 sidecar 覆盖 unsupported Lingo bytecode、diagnostic 去重与 unsupported Lingo bytecode 阻断、重复 route 优先保留 reader source-map evidence、`tsuinosora.route_graph_report.v1` covered route 派生、未解析或不可读 Director/Shockwave container 阻断、`tsuinosora.visual_reference_report.v1`、`tsuinosora.asset_analysis.v1`、脚本引用位置、container source、use timing、visible bbox、edge padding、颜色分布、重复 hash、reference match、classification count、`character_atlas` 切片、低置信度 quarantine、分类冲突 quarantine、`tsuinosora.conversion_report.v1`、`tsuinosora.modern_profile_report.v1`、`tsuinosora.mount_policy.v1`、`tsuinosora.stage3_gate_report.v1`、`tsuinosora.nativevn_package_input_report.v1` 文件 manifest/hash evidence、`tsuinosora.local_gate_report.v1`、`tsuinosora.demo_slice_report.v1`、derived route count、route choice 保真、route-bound cast member 到 native-assets 的 hash-bound `mount_assets` 派生、route scenario refs、patch Web scenario refs、patch Windows route-bound `mount_assets` scenario refs 和 Asset analysis role 白名单、modern feature fallback/switch/core-state 阻断、缺源 blocking diagnostic、explicit routes 在 local/demo gate 中 blocking，以及 synthetic source/unpacked/routes/features pass；`cargo test -p astra-release --test release_report tsuinosora` 覆盖 TsuiNoSora package section release gate 的缺 section、quarantine、route coverage 缺口、路径泄露、payload-like field 泄露、modern report、formal release `manual_signoff` required check set 缺失和未通过阻断；`cargo test -p astra-cli --test target_platform tsuinosora_synthetic_gate_runs_internal_and_patch_player_routes` 覆盖公开 synthetic TsuiNoSora project 的 internal classic/modern headless、Windows player、Web player 以及 patch classic/modern headless、Windows player、Web player 全路线自动化，bundle 内 `AstraPlayer.mount_policy.json`、`player.mount_policy`、`player.mount_policy_hash`、Windows `mount_probes` + `--mount-root` 的 `player.patch_mount_probe`、route-bound `player.patch_mount_asset`、invalid role blocking 和 patch target 的 `player.patch_direct_read`，并阻断 mount policy 文件被篡改但 alias 语义未变的情况；`cargo test -p astra-cli --test target_platform tsuinosora_demo_slice_generates_playable_nativevn_and_player_routes` 覆盖由 `demo-slice --config` 生成的 NativeVN project、package section 清洗、`vn.commercial_baseline`、internal classic/modern Windows/Web bundle route 和 patch classic/modern Windows bundle direct-read；仍缺完整 Director/Shockwave cast parser/source-map reader、真实商业全量 route extraction、真实商业 NativeVN payload 写入和正式 release signoff |

## Stage 6 平台完成项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S6-LINUX-HOST-01` | `SPEC_READY` | 真实 Linux host smoke、platform decode、audio、save store、IME/gamepad 和 release evidence 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-MACOS-HOST-01` | `SPEC_READY` | 真实 macOS AppKit/winit、Metal/wgpu、CoreAudio、AVFoundation、App Support 和 notarization capability 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-IOS-HOST-01` | `SPEC_READY` | Swift/SwiftUI launcher、Metal surface、safe area/touch、AVAudio/AVFoundation、app container save 和 no-JIT Luau gate 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-ANDROID-HOST-01` | `SPEC_READY` | Kotlin/Java launcher、Vulkan/wgpu surface、AAudio、MediaCodec、SAF/package import、activity resume 和 no-JIT Luau gate 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-LINUX-PLAYER-AUTOMATION-01` | `SPEC_READY` | Linux player input automation、window focus、native input、frame region、audio meter 和 route evidence 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-MACOS-PLAYER-AUTOMATION-01` | `SPEC_READY` | macOS player input automation、AppKit/winit focus/input、frame region、CoreAudio meter 和 route evidence 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-IOS-PLAYER-AUTOMATION-01` | `SPEC_READY` | iOS player touch/keyboard automation、safe area、AVAudio meter、resume 和 route evidence 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-ANDROID-PLAYER-AUTOMATION-01` | `SPEC_READY` | Android player touch/keyboard automation、activity lifecycle、audio focus、frame region 和 route evidence 见 [Stage 6](stages/stage-6-platform-completion.md) |

## Stage 7 AstraRPG 规划项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S7-POLICY-01` | `SPEC_READY` | Shared Luau policy runtime 迁移计划见 [AstraRPG migration](../migrations/astra-rpg-design-alignment-migration.md) 和 [Stage 7](stages/stage-7-astra-rpg.md)；`Engine/Source/Runtime/astra-policy/` 尚不存在 |
| `S7-RPG-PROVIDER-01` | `SPEC_READY` | `AstraRpgRuntimeProvider` 目标、release checks 和 editor metadata 见 [AstraRPG contract](../contracts/rpg-trpg.md)；provider crate 尚不存在 |
| `S7-RPG-CORE-01` | `SPEC_READY` | `RpgSession`、`RpgIntent`、`RpgEffect`、`RpgSheet` 和 `CommittedAgentOutput` DTO 见 [AstraRPG contract](../contracts/rpg-trpg.md)；core crate 尚不存在 |
| `S7-RPG-POLICY-01` | `SPEC_READY` | `astra.rpg.*` host API、policy bundle 和 capability gate 见 [AstraRPG runtime blueprint](../implementation/astra-rpg-runtime.md)；policy crate 尚不存在 |
| `S7-RPG-AI-TOWN-01` | `SPEC_READY` | AI Town 20 NPC one-day headless gate 见 [AI Town sample](../samples/astra-rpg-ai-town/README.md)；sample project 尚不存在 |
| `S7-RPG-TRPG-01` | `SPEC_READY` | `rpg.trpg` ruleset/profile、dice/check/ruling/transcript/seat authority 见 [AstraRPG contract](../contracts/rpg-trpg.md)；不创建顶层 AstraTRPG 模块 |
| `S7-RPG-CP2020-01` | `SPEC_READY` | CP2020 local-private adapter 边界见 [CP2020 local adapter sample](../samples/astra-rpg-cp2020-local-adapter/README.md)；不能提交规则正文、表格或 payload |
| `S7-RPG-GATE-01` | `SPEC_READY` | `runtime_provider.astra_rpg`、`rpg.*`、`rpg.trpg.*` 和 `rpg.cp2020.local_private_adapter` release check 见 [release checks](../implementation/release-gate-checks.md)；release tests 尚不存在 |

## Stage 8 AstraRPG Network 规划项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S8-RPG-NET-CONTRACT-01` | `SPEC_READY` | `rpg.net.*` protocol DTO 见 [Stage 8](stages/stage-8-astra-rpg-network.md)；protocol crate 尚不存在 |
| `S8-RPG-NET-SERVER-01` | `SPEC_READY` | server-side session authority、seat assignment 和 redacted audit 见 [Stage 8](stages/stage-8-astra-rpg-network.md)；server crate 尚不存在 |
| `S8-RPG-NET-CLIENT-01` | `SPEC_READY` | client-side transcript sync、local view 和 reconnect cursor 见 [Stage 8](stages/stage-8-astra-rpg-network.md)；client crate 尚不存在 |
| `S8-RPG-NET-REPLAY-01` | `SPEC_READY` | network replay gate 见 [release checks](../implementation/release-gate-checks.md)；release test 尚不存在 |

## 下一步实施顺序

| Order | Work | Status | Why now |
| --- | --- | --- | --- |
| 1 | `S2-PACKAGE-01` package container | `DONE` | `astra-package` 提供共享 container、Zstd codec、crypto descriptor、bounded reader；Runtime save 已迁移 |
| 2 | `S2-ASSET-01` + `S2-ASSET-02` asset/import/cook | `IN_PROGRESS` | sidecar、registry、metadata import、单资产 DDC key 已存在；生产加固正在补依赖图、持久缓存、bounded concurrency、取消和原子批次提交 |
| 3 | `S2-GATE-01` release report | `DONE` | `astra-release` 和 `astra package validate` 输出 `astra.release_report.v1`；release profile 缺 `compiled.project` cook/project artifact 时阻断 |
| 4 | `S2-MEDIA-01` 到 `S2-MEDIA-05` media providers | `DONE` | `astra-media` 提供 headless renderer、TextLayout、AudioGraph、FilterGraph、DecodeProvider 和 optional native feature gates |
| 5 | `S2-HEADLESS-*` Migration 11 | `SPEC_READY` | 先建立测试专用完整 host、真实 PNG/WAV、序列化物理输入和全 Runtime test 收束，再把它设为真实平台验收强制 preflight |
| 6 | `S2-WINDOWS-HOST-01` + `S2-WINDOWS-WMF-01` + `S2-WINDOWS-GATE-01` Windows platform repair | `IN_PROGRESS` | Windows real host 已接入 winit/wgpu/WASAPI/WMF/Saved Games/package range；Player 全服务接线和同 run conformance/automation evidence 尚未完成 |
| 7 | `S3-MODULE-LAYOUT-01` AstraVN module layout | `DONE` | AstraVN 已迁到 `Engine/Source/Modules/AstraVN/`，workspace/path dependency 已改链 |
| 8 | `S3-CRATE-SPLIT-01` AstraVN functional crate split | `DONE` | AstraVN 功能 crate 已拆出，`astra-vn` 只作为 facade、`rlib`/Rust ABI `dylib` 和兼容 re-export |
| 9 | `S3-DYLIB-01` AstraVN Rust dylib target | `DONE` | facade-only dylib smoke 和 VN extension fixture 已通过 |
| 10 | `S3-SCRIPT-01` + `S3-SCRIPT-02` `.astra` compiler frontend | `IN_PROGRESS` | Migration 6 focused implementation 已关闭；Stage 3 状态等待同 package Windows/Web formal Player evidence |
| 11 | `S3-ADVANCED-01` Advanced presentation opt-in profile | `IN_PROGRESS` | 技术内容已合并到 NativeVN；等待 formal Windows/Web advanced presentation evidence |
| 12 | `S3-GAME-TARGET-01` NativeVN Game target | `IN_PROGRESS` | NativeVN sample cook/package/full playthrough、VN package sections、official policy source cache、standard command/presentation provider release gate、Scenario DSL `complete_wait` slice、Windows/Web standalone bundle 和 route report slice 已落地；仍需 `S3-PLAYER-AUTOMATION-01` 证明真实 player input/render/audio/output |
| 13 | `S3-PLAYER-AUTOMATION-01` Windows/Web live player automation | `IN_PROGRESS` | `astra-player-core`/`astra-player` 和 `player.full_playable` release check 已落地；Windows runner 已能启动 bundle live window、执行 `SendInput`、捕获 player host `TRACE` consumed log、把 `input_consumption` hash/count 写入 transcript、采集视觉 region hash、绑定视觉 comparison、计算 package 音频 meter 并输出 live report。下一步是让 player 对真实输入推进可见 VN state 并覆盖 full route，同时补 Chrome/Edge CDP host run；两端都要求 input transcript、host consumed trace、视觉 region hash 变化、视觉截图对比、音频 meter、host evidence 和 route evidence 同 run 产出 |
| 14 | `S3-TSUI-INTERNAL-DEMO-01` TsuiNoSora internal classic full-resource demo bundle | `IN_PROGRESS` | 本期只覆盖 `tsuinosora-internal-game` 的 `classic` profile，但完成条件是全量资源转换、原体验还原和 100% 可玩：ProjectorRays 本地 dump adapter、full dump coverage report、converted resources sidecar 校验、JSON-backed metadata chunk converter、empty `Lscr` no-op metadata converter、8bpp `BITD` palette sidecar converter、KEY-bound `ediM` `MACRZ` verified MP3 converter、ProjectorRays route/source-map 派生、NativeVN asset sidecar、cooked asset package section、scenario refs package section、`asset.vfs_manifest`/`asset.catalog` package VFS evidence、同一 `.astrapkg` 派生 Windows/Web bundle、原始分辨率 display config、Windows live window、live automation report release check 入口和视觉截图对比阻断 gate 已落地；当前私有 full dump 已有 2527/2527 converted evidence，Windows/Web bundle manifest 已从同一 package 产出，原版/Demo title checkpoint 视觉对比可通过，Windows live automation 可生成 trace log，28 次 `SendInput` 均有 player host consumed trace 并被 release gate 读取。剩余阻断是 Demo live input 后可见状态不变化、28 条 full classic route 尚未由真实 player automation 覆盖、required manual signoff 尚未提供。`modern`、Patch-only、Runtime Patch/VFS 插件移出本期 |
| 15 | `S3-TSUI-GATE-01` TsuiNoSora commercial validation gate | `IN_PROGRESS` | 脱敏 inventory、direct-readable extract preflight、Director `imap`/`mmap` resource map preflight、free mmap entry 计数、受限 `XFIR` RIFF/RIFX exact wrapper reader、opaque/compressed `XFIR` 与尾随未验证 bytes reader-required 阻断、Director `KEY*`/`CAS*` cast map preflight、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight、ProjectorRays 本地 dump adapter、`tsuinosora.script_source_map.v1` 脱敏 reader sidecar、reader id/hash/output contract evidence、script source map bytecode 阻断、route graph report、reference report、Asset analysis helper、native asset sidecar/cook/package registry、`stage3-gate`/`local-gate` orchestrator、NativeVN package input writer、project-level `package_sections`、package section release gate、公开 synthetic internal/patch player route gate、patch Web direct-read route check 和 Windows route-bound `mount_assets` local asset check 已落地；下一步接完整 Director/Shockwave cast parser/source-map reader、真实商业 full slice acceptance、正式 release signoff 和后续 patch/VFS 计划 |

补充：`S3-TSUI-GATE-01` 的 Director Lingo preflight 还覆盖 `Lctx` entry count/table hash；`Lctx` 与 `Lnam` 都不输出 table payload 或 name/context 字符串。Malformed `Lctx` table 或未终止 `Lnam` table 必须 blocking。

补充：`S3-TSUI-GATE-01` 的 Director cast preflight 还覆盖重复 `CASt` binding blocking；同一个 `CASt` 被多个 `CAS*` library/slot 引用时不能继续作为唯一 route/source-map evidence。

补充：`S3-TSUI-GATE-01` 的 cast source-map report 还覆盖手写 `tsuinosora.cast_map.v1` 和外部 `tsuinosora.director_cast_map.v1` sidecar 的 payload/正文/bytecode 字段阻断，避免素材映射 evidence 携带商业内容。

补充：`S3-TSUI-GATE-01` 的 release gate 还覆盖进入 package 的 `tsuinosora.*` section payload-like 字段阻断；正文、脚本文本、bytecode、payload body 或 source payload 字段会输出 `ASTRA_TSUI_REPORT_PAYLOAD_LEAK`，只有 `redaction.payload: omitted` 允许存在。

补充：`S3-TSUI-GATE-01` 的 release gate 还覆盖空 `tsuinosora.asset_analysis` 阻断；`status: pass` 但 `assets: []` 不能作为 Asset Analysis Gate 完成证据。

补充：`S3-TSUI-GATE-01` 的 release gate 还覆盖空 `tsuinosora.conversion_manifest.resources` 阻断；route 全部 covered 但没有 converted resource evidence 时会输出 `ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE`。resource 缺 source/native 相对路径、classification、source hash、converted hash 或正 byte size 时也会 blocking。

补充：`S3-TSUI-GATE-01` 的 Director cast preflight 还覆盖显式 `tsuinosora.director_cast_member_metadata.v1` 读取；只允许 kind、route id、command id、anchor、bounds 和 metadata hash 进入 report，并会传递到 `tsuinosora.cast_source_map_report.v1`。

补充：`S3-TSUI-GATE-01` 的 Director cast metadata layout 还覆盖 anchor/bounds 数值校验；anchor 非数值或 bounds 负尺寸会 blocking。

补充：`S3-TSUI-GATE-01` 的 Director cast metadata 还覆盖 `character_atlas` parts 读取和传递；缺 parts、part symbol/crop/anchor/state 不合规都会 blocking。

补充：`S3-TSUI-GATE-01` 的视觉参考 evidence 还覆盖默认 `Title.png`/`Game.png` 的固定尺寸和 hash；缺文件、PNG 不可读、hash mismatch 或 dimensions mismatch 会让 `tsuinosora.visual_reference_report.v1`、Stage 3 gate 和 release gate blocking。

补充：`S3-TSUI-GATE-01` 的 route graph/script source-map report 还覆盖重复 `route_id` 冲突 blocking；同一 `route_id` 指向不同 terminal/choice signature 时不能进入 NativeVN package input。

补充：`S3-TSUI-GATE-01` 还覆盖同一 route 内重复 choice id blocking；重复 choice 不能生成重复 `player_input choose` 或 `.astra` option key。

补充：`S3-TSUI-GATE-01` 还覆盖 NativeVN package input 对显式 route 输入的写入前校验；不安全 symbol、非 covered coverage、重复 choice 或冲突 route signature 会阻断，并且不会写出 story/scenario refs。

补充：`S3-TSUI-GATE-01` 还覆盖坏 route graph sidecar 不能被 script source-map fallback 绕过；只有 route graph 缺失时才允许 fallback。

补充：`S3-TSUI-GATE-01` 的 `tsuinosora.script_source_map.v1` 还要求 unsupported `Lscr` bytecode route 绑定匹配的 `script_resource_id` 和 `script_payload_sha256`；缺字段、未知 resource 或 Lscr hash mismatch 都会 blocking，且不写 bytecode 或脚本文本。

补充：`S3-TSUI-GATE-01` 还要求 unsupported Lingo bytecode 的 route coverage 覆盖每个 `Lscr` resource；只覆盖同一 `director_lingo_map.json` 的部分 resource 会 blocking。

补充：`S3-TSUI-GATE-01` 的 `local-gate` 现在会阻断显式 routes 输入，要求真实本地 gate 从 route graph 或 script source-map report 派生 route coverage；被阻断时不写 `tsuinosora.nativevn_package_input_report.v1`。

补充：`S3-TSUI-GATE-01` 的 Director resource map preflight 现在要求 RIFF/RIFX declared size 精确匹配可读文件大小；过大、过小或尾随未验证 bytes 都会 blocking，并且不记录 resource/tag coverage，避免把 container 之外的数据误当作素材。

| 15 | `S2-VFS-01` Asset VFS Provider URI slice | `DONE` | `asset.vfs_manifest`、`asset.catalog`、single `vfs_provider` slot、overlay whiteout 和旧 `asset.registry` blocking 已落地；legacy pack reader 留 Stage 5 |
| 16 | `S3-RUNTIME-PROVIDER-01` NativeVN gameplay runtime provider | `DONE` | `astra-vn-runtime-provider`、真实 FFI instance/session lifecycle、RuntimeWorld StateMachine action、provider.policy binding、behavioral release gate 和 scenario runner provider path 已落地 |
| 17 | `S4-PLUGIN-01` Plugin Manager UI | `REOPENED_SPEC` | Plugin Manager 需要显示和修改 Stage 1/2 的 enablement、dependency graph、VFS mount provider 和 gameplay runtime provider binding |
| 18 | `S4-EDITOR-RUNTIME-PROVIDER-01` Editor runtime provider switching | `REOPENED_SPEC` | Editor shell 需要读取 `RuntimeEditorMetadata`，切换 NativeVN surfaces，并把 provider/profile 传给 PIE 和 Release Gate |
| 19 | `S4-AI-01` 到 `S4-GATE-01` AI/MCP closure，包括 `S4-AI-ONNX` 和 `S4-AI-VFS-01` | `REOPENED_SPEC` | Runtime Director、provider profile、ONNX ModelBundle、Asset VFS/encryption 复用、memory、Context Pack、AI Control 和 release gate 需要一起落地 |
| 20 | `S4-EDITOR-TARGET-01` AstraEditor Editor target | `REOPENED_SPEC` | Editor target 需要 Qt/QML shell、PIE bridge 和 gameplay runtime provider selector |
| 21 | `S5-GAME-RUNTIME-01` + `S5-EMUCORE-SM-01` + `S5-LEGACY-VFS-01` | `REOPENED_SPEC` | AstraEMU 先接成 `AstraEmuRuntimeProvider`，再把 legacy VM 映射为 family-private scheduler/context/basic-block/action 状态机，并复用 Asset VFS legacy pack mount |
| 22 | `S5-MANAGER-01` + `S5-PROGRAM-TARGET-01` + `S5-FAMILY-01` + `S5-AUTOPROBE-01` + `S5-SCRIPT-01` + `S5-TEXT-01` + `S5-FILTER-01` | `REOPENED_SPEC` | AstraEMU Manager 仍作为 Program target，启动 gameplay runtime session、family plugin，并复用 Stage 4 provider、MCP 和 memory |
| 23 | `S6-LINUX-HOST-01` + `S6-MACOS-HOST-01` + `S6-IOS-HOST-01` + `S6-ANDROID-HOST-01` platform completion | `SPEC_READY` | Windows/Web 之外的平台完成从 Stage 2 移出，等 VN/Core/Editor 发布路径稳定后集中接入真实 SDK evidence |
| 24 | `S7-POLICY-01` + `S7-RPG-PROVIDER-01` + `S7-RPG-CORE-01` + `S7-RPG-POLICY-01` + `S7-RPG-AI-TOWN-01` + `S7-RPG-TRPG-01` + `S7-RPG-CP2020-01` + `S7-RPG-GATE-01` | `SPEC_READY` | AstraRPG 需要先抽通用 Luau policy，再接 RPG provider/core/policy，最后用 AI Town、`rpg.trpg` 和 CP2020 local-private adapter gate 验证；当前只完成设计和迁移计划 |
| 25 | `S8-RPG-NET-CONTRACT-01` + `S8-RPG-NET-SERVER-01` + `S8-RPG-NET-CLIENT-01` + `S8-RPG-NET-REPLAY-01` | `SPEC_READY` | Server/Client protocol 依赖 Stage 7 本地 seat/transcript/replay 语义稳定；当前只完成协议阶段设计，不阻塞 Stage 7 |

## 验证命令

```bash
python Tools/check_docs.py
cargo fmt --check
python Tools/run_cargo_isolated.py clippy --workspace --all-targets -- -D warnings
python Tools/run_cargo_isolated.py test --workspace
git diff --check
```

Expected output: docs check reports checked markdown files；fmt/clippy/workspace tests pass；diff check has no whitespace errors。

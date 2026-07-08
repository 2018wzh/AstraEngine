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
| Stage 1 EngineCore | `DONE` | `cargo test --workspace` 通过；覆盖 core、runtime、plugin、property、Target manifest、headless scenario |
| Stage 2 Media + Package | `IN_PROGRESS` | 既有 Asset/Cook/Package、headless media、release report、Target manifest、strict scenario runner、flat StateMachine、Await/Fence、Windows product host evidence 和 Web browser evidence 继续有效；完成边界只覆盖 Windows/Web，Linux/macOS/iOS/Android 移到 Stage 6。Stage 2 因 VFS contract 重开：当前 `astra-asset` 还未作为 package/local authorized/legacy pack/overlay mount 的完整 contract owner 落地，`astra-package` 仍只是 package-backed source。已验证 `cargo test -p astra-runtime --test state_machine_tick`、`cargo test -p astra-runtime --test await_token`、`cargo test -p astra-test --test native_smoke`、`cargo test -p astra-platform-windows`、`cargo test -p astra-platform-web`、`cargo test -p astra-platform-web --target wasm32-unknown-unknown --no-run`、`wasm-pack test --headless --chrome Engine/Source/Platform/astra-platform-web`、`cargo test -p astra-media decode_provider`、`cargo test -p astra-release release_report` 和 `cargo test -p astra-cli --test target_platform`；既有 evidence 绑定 `Engine/Fixtures/PublicDomainMedia/manifest.json`、`decode.wmf.audio`、`decode.wmf.video_first_frame`、`renderer.wgpu_surface`、`save.known_folder_rw`、`decode.browser_media`、`save.web_storage_rw` 和 `package.web_source_read` |
| Stage 3 AstraVN | `IN_PROGRESS` | 单 crate `astra-vn` 已开始落地，覆盖 `.astra` parser/compiler、duplicate id / target / reachability blocking diagnostic、`CompiledStory`、Story/Variable/Command manifest、基础 `VnRuntimeState`、call/return stack、route flags、backlog/read-state/voice replay、Runtime save container 中的 `vn.runtime_state`/`vn.policy_state`、save/load、auto/skip/config/unlock system state、Luau sandbox、mutation trace、rollback scope/playback、command/query/trace capability 与 serializable snapshot policy、policy bundle manifest/source cache/hash gate、VN extension binding manifest、standard command manifest、commercial baseline manifest feature gate、presentation provider manifest/filter fallback/wait capability gate、movie/voice/timeline await state slice、StageModel/VideoLayer/AudioCommand/Timeline lifecycle slice、headless presentation execution、FilterGraph CPU execution、AdvancedVN opt-in profile、SystemStoryManifest 必需入口和 policy binding、Graph/Timeline metadata validation、NativeVN sample cook/package/full playthrough、Windows/Web bundle route report、VN package sections、VN Scenario DSL slice、route-bound `mount_assets` 和 `vn.*` release gate。Stage 3 因 module layout、crate split 和 GameRuntime contract 重开：AstraVN 要迁到 `Engine/Source/Modules/AstraVN/`，拆成多个功能 crate，并让 `astra-vn` 只作为 facade；随后新增 `NativeVnRuntimeProvider` 对齐，不能把 VN Core 写成所有玩法的基类；Windows/Web live player automation、TsuiNoSora 完整 Director/Shockwave cast parser/source-map reader、真实本地 conversion gate 和真实本地 patch direct-read 仍未完成 |
| Stage 4 Editor + AI/MCP | `REOPENED_SPEC` | Editor workflow、runtime-provider-aware shell、Plugin Manager、AI provider profile、ONNX ModelBundle、Runtime Director、memory、MCP context 和 AI/MCP gate 已写入文档；`Editor/Source` 和 `Engine/Plugins/Providers/astra-ai-onnx` 尚不存在。Stage 4 因 VFS/GameRuntime contract 重开，Project Wizard、PIE、Debugger 和 Release Gate 必须读取 `RuntimeEditorMetadata`，ONNX ModelBundle、Context Pack、generated artifact 和 MCP package access 需要改为统一 VFS mount evidence |
| Stage 5 AstraEMU | `REOPENED_SPEC` | `Docs/contracts/astraemu-ipc.md`、`Docs/implementation/astraemu-legacy-runtime-framework.md`、`Docs/implementation/emulator-core-state-machine.md` 和 `Docs/emu` 已写清 `AstraEmuRuntimeProvider`、`LegacyRuntimeProvider` facade、VM 状态机映射、auto probe、Trusted Luau、文本翻译、FilterGraph preset 和 legacy pack VFS mount；`AstraEMU/Source` 尚不存在，不能把 full-family runtime 写成已实现 |
| Stage 6 Platform Completion | `SPEC_READY` | Linux、macOS、iOS 和 Android 的真实 host smoke、launcher、platform decode、save/resume 和 release evidence 已移到 [stage-6-platform-completion](stages/stage-6-platform-completion.md)；当前只保留 capability crate 和 planned gate |

Stage 3 补充证据：TsuiNoSora 本地 helper 已生成 `tsuinosora.projectorrays_reader_report.v1`、`tsuinosora.projectorrays_full_dump_report.v1`、脱敏 `tsuinosora.script_source_map.v1` 和 `tsuinosora.script_source_map_report.v1`，ProjectorRays 只作为 ignored config 指向的本地工具来源，report 不写脚本文本、payload、bytecode、原始 dump 或本地路径；在 `demo-slice` 中，ProjectorRays pass report 只满足 Director reader-required preflight，不会消除 payload、路径、hash、resource 断裂或 route/source-map 缺失 diagnostic。Full-resource classic playable bundle 现在要求 `projectorrays_full_dump_report.resource_coverage` 覆盖每个 ProjectorRays binary chunk；该 report 已输出 `chunk_fourcc_counts` 和 `conversion_plan`，用于按 `BITD`、`STXT`、`snd `、`ediM`、`XMED`、`CASt`、`Lscr` 等类别推进真实转换。当前私有 dump 覆盖 2527 个 binary chunk，但 converted resource evidence 为 0，因此 internal bundle 入口会在构建 package 前 blocking。Asset analysis pass 后会真实写入 `native-assets/`，生成 asset sidecar，把 source/native 相对路径、classification、source hash、converted hash 和 byte size 汇入 `tsuinosora.conversion_report.v1`；`astra-cli cook` 会读取 `nativevn.asset_roots`，经 `astra-cook` 写入 cooked asset package section，并在 `asset.registry.assets` 中登记 path、role、section、hash 和 byte size。NativeVN package input 已把 route graph/source map 的 sanitized choice id 保留到 `.astra` option key 和 scenario `player_input choose`，多 choice route 不再压缩成单个 synthetic choice；`stage3-gate` 派生的 route-bound cast member 会通过 source/native hash 写入 `mount_assets`，并在没有显式 routes 入参的 `local-gate` 中继续进入 conversion report 和 patch/windows scenario refs。`demo-slice --config` 已能从私有 root config 调用 `local-gate`，生成 `tsuinosora.demo_slice_report.v1`、可 cook/package 的 NativeVN project、脱敏 package sections、Windows/Web standalone bundle 输入和 player route scenarios；`demo-config-template` 和 `Examples/TsuiNoSora/Docs/demo.config.template.json` 提供 repo-relative `.local` scaffold，不写本地绝对路径。`astra-player-core`、`astra-player` 和 `astra-release` 已落地 `astra.player_automation_script.v1`、`astra.player_input_transcript.v1`、`astra.player_automation_report.v1` 与 `player.full_playable` evidence 校验；默认 package validate 不会把 route report 当成 full playable，只有显式传入匹配 package hash/profile/target 的 live automation report 才能通过该 check。Stage 3 仍不能标 `DONE`。

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
| `S1-DYLIB-01` | `DONE` | `cargo test -p astra-engine dylib_facade` |
| `S1-PLUGIN-01` | `DONE` | `cargo test -p astra-plugin descriptor_gate` and `cargo test -p astra-plugin load_unload` |
| `S1-PLUGIN-02` | `DONE` | `cargo test -p astra-plugin ffi_action_provider` |
| `S1-PLUGIN-03` | `DONE` | `cargo test -p astra-plugin extension_registry` |
| `S1-PROP-01` | `DONE` | `cargo test -p astra-property --test property_metadata` and `cargo test -p astra-property --test expand_smoke` |
| `S1-TEST-01` | `DONE` | `cargo test -p astra-test native_smoke` |
| `S1-OBS-01` | `DONE` | `cargo test -p astra-cli --test logging` |
| `S1-TARGET-01` | `DONE` | `cargo test -p astra-target` and `cargo test -p astra-cli --test target_platform` |

## Stage 2 平台修复项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S2-PLATFORM-01` | `DONE` | `cargo test -p astra-platform`；共享 report schema、SDK 分层和 required smoke validation 已落地 |
| `S2-WINDOWS-HOST-01` | `DONE` | `cargo test -p astra-platform-windows`；Windows probe 输出 hidden window、`renderer.wgpu_surface`、`audio.wasapi`、`save.known_folder_rw`、XInput 和 SDK 状态 |
| `S2-WINDOWS-WMF-01` | `DONE` | `cargo test -p astra-media decode_provider`；public media manifest 校验固定 sha256，WMF `decode.wmf.audio` 输出 MP3 bounded PCM CPU buffer，`decode.wmf.video_first_frame` 输出 MP4 BGRA 首帧，invalid video 返回 `ASTRA_WMF_DECODE` |
| `S2-WINDOWS-GATE-01` | `DONE` | `cargo test -p astra-release release_report` and `cargo test -p astra-cli --test target_platform`；缺 required Windows smoke 会阻断 release check |
| `S2-PLUGIN-GATE-01` | `DONE` | `cargo test -p astra-package package_roundtrip` and `cargo test -p astra-release release_report`；package 写入 `plugin.extension_registry` 和 `plugin.dependency_graph`，release gate 阻断 unresolved conflict、missing binding 和 unresolved dependency |
| `S2-RUNTIME-FSM-01` | `DONE` | `cargo test -p astra-runtime --test state_machine_tick`；flat FSM validation、terminal/completed、priority conflict diagnostic 和 Always tick trigger 已落地 |
| `S2-RUNTIME-AWAIT-01` | `DONE` | `cargo test -p astra-runtime --test await_token`；Await timeout materialization、unknown/duplicate result diagnostic 和 pending token serialization 已落地 |
| `S2-SCENARIO-GATE-01` | `DONE` | `cargo test -p astra-test --test native_smoke`；unknown VN action/assertion blocked、declared package missing blocked、Stage 1 native smoke pass |
| `S2-WEB-HOST-01` | `DONE` | `cargo test -p astra-platform-web`、`cargo test -p astra-platform-web --target wasm32-unknown-unknown --no-run`、`wasm-pack test --headless --chrome Engine/Source/Platform/astra-platform-web`、`cargo test -p astra-media decode_provider --target wasm32-unknown-unknown --no-run`、`cargo test -p astra-release release_report` 和 `cargo test -p astra-cli --test target_platform`；Web required smoke 覆盖 `renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw` 和 `package.web_source_read` |
| `S2-VFS-01` | `REOPENED_SPEC` | [Asset VFS Contract](../contracts/asset-vfs.md) 和 [Asset VFS Blueprint](../implementation/asset-vfs.md) 已定义 package/local authorized/legacy pack/overlay mount、`VfsMountProvider`、resolve report 和 release gate；现有代码尚未迁移到完整 VFS mount family |

## Stage 3 进行中项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S3-MODULE-LAYOUT-01` | `REOPENED_SPEC` | [AstraVN Module Layout Migration](../migrations/astra-vn-module-layout-migration.md) 已定义 `Engine/Source/Modules/AstraVN/astra-vn` 目标路径；当前代码仍在单 crate 路径，尚未执行真实搬迁 |
| `S3-CRATE-SPLIT-01` | `REOPENED_SPEC` | [AstraVN Crate Split Migration](../migrations/astra-vn-crate-split-migration.md) 已定义 `astra-vn-script`、`astra-vn-core`、`astra-vn-policy`、`astra-vn-presentation`、`astra-vn-commands`、`astra-vn-system`、`astra-vn-save`、`astra-vn-package`、`astra-vn-plugin`、`astra-vn-editor`、`astra-vn-runtime-provider` 和 facade-only `astra-vn`；尚未执行真实拆分 |
| `S3-DYLIB-01` | `REOPENED_SPEC` | 旧 `cargo test -p astra-vn --test vn_dylib_facade` 和 `cargo test -p astra-vn --test vn_plugin_extensions` 只证明 monolithic `astra-vn` facade；新目标要求 `astra-vn` 成为 facade-only crate 并 re-export AstraVN 子 crate |
| `S3-RUNTIME-PROVIDER-01` | `REOPENED_SPEC` | [Game Runtime Provider Contract](../contracts/game-runtime-provider.md) 和 [Game Runtime Provider Blueprint](../implementation/game-runtime-provider.md) 已定义 `NativeVnRuntimeProvider`；现有 facade、VN extension manifest、package sections 和 release checks 尚未迁移到 `astra-vn-runtime-provider` |
| `S3-SCRIPT-01` | `DONE` | `cargo test -p astra-vn --test compiler_runtime`、`cargo test -p astra-vn --test commercial_baseline` 和 `cargo test -p astra-vn --test compiler_diagnostics` 覆盖 `.astra` story/state/scene/text/choice/system page option/jump/call/return/mutate 解析、source id、quote、arrow、indent、duplicate attr、孤立 option、缺 key、未知 system page、重复 explicit source id、重复 text key、非法变量域、未定义 route target 和 unreachable main state 阻断 |
| `S3-SCRIPT-02` | `DONE` | `cargo test -p astra-vn --test compiler_runtime` 和 `cargo test -p astra-vn --test compiler_diagnostics` 覆盖 `CompiledStory`、StoryManifest、VariableManifest、CommandManifest、`SystemStoryManifest` compiler output、source map、debug symbols、route graph、stable hash 和 main story reachability diagnostic |
| `S3-CORE-01` | `DONE` | `cargo test -p astra-vn --test compiler_runtime`、`cargo test -p astra-vn --test commercial_baseline`、`cargo test -p astra-vn --test system_controls`、`cargo test -p astra-runtime --test trigger_event` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 dialogue、choice、变量 mutate、call/return stack、skip-read、auto/skip config state、route coverage、可序列化 route flags 和 StateMachine trigger event |
| `S3-CORE-02` | `DONE` | `cargo test -p astra-vn --test compiler_runtime`、`cargo test -p astra-vn --test system_controls` 和 `cargo test -p astra-vn --test vn_save_container` 覆盖 rich `BacklogEntry`、read-state、voice replay、skip-read eligibility、`VnReplayUiState` replay UI snapshot、replay UI hash 和 save/load roundtrip |
| `S3-CORE-03` | `DONE` | `cargo test -p astra-vn --test compiler_runtime`、`cargo test -p astra-vn --test vn_save_container` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 VN save/load、Runtime save container extra sections、`vn.runtime_state`、`vn.policy_state`、backlog/read-state/voice replay/route flags/变量/pending wait、Luau serializable snapshot、mutation replay event metadata 和 replay hash |
| `S3-LUAU-01` | `DONE` | `cargo test -p astra-vn --test luau_sandbox` 和 `cargo test -p astra-vn --test luau_mutation` 覆盖 `mlua` Luau sandbox、`astra.var`、`astra.mutate.set_var` trace、previous value、rollback scope/playback、`astra.command` register/filter/emit/enqueue 记录型 capability、`astra.query` text/asset/backlog/savepoint/layout、`astra.trace` event/performance scope、serializable `astra.snapshot`、不可序列化 snapshot/command/trace payload blocking diagnostic、direct table write ignored 和 capability denial |
| `S3-LUAU-02` | `DONE` | `cargo test -p astra-vn --test policy_bundle` 和 `cargo test -p astra-release --test release_report release_gate_` 覆盖官方 `astra.policy.standard` Luau source cache、真实 source hash/byte size、`vn.policy_bundle_manifest`、`vn.policy_bundle_source_cache`、required capabilities、package lock、缺 source cache blocking diagnostic、hash mismatch blocking diagnostic 和 sandbox 执行注册 command/trace/snapshot |
| `S3-PLUGIN-01` | `DONE` | `cargo test -p astra-vn --test vn_plugin_extensions` 和 `cargo test -p astra-release --test release_report release_gate_` 覆盖 Luau policy bundle provider、VN command provider、presentation command provider、Graph/Timeline metadata extension、release check provider 显式绑定、缺失 binding 和重复 binding 阻断 |
| `S3-PRESENT-01` | `DONE` | `cargo test -p astra-media --test filter_graph`、`cargo test -p astra-vn --test presentation_execution`、`cargo test -p astra-vn --test commercial_baseline`、`cargo test -p astra-vn --test presentation_model`、`cargo test -p astra-vn --test advanced_presentation`、`cargo test -p astra-vn --test await_gates`、`cargo test -p astra-vn --test standard_command_manifest`、`cargo test -p astra-release --test release_report release_gate_` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 StageModel、Layer、Camera、TextWindow、VideoLayerState、AudioCommand、PresentationTimeline hash、timeline complete/cancel/replace lifecycle、standard command manifest/usage gate、presentation provider manifest、FilterGraph CPU execution、headless presentation execution、filter fallback policy、movie/voice/timeline wait capability、serializable `VnWaitState`、real player `complete_wait` slice 和 `VnAdvancedPresentationManifest` evidence |
| `S3-ADVANCED-01` | `DONE` | `cargo test -p astra-vn --test advanced_presentation` 和 `cargo test -p astra-cli --test target_platform advanced_vn_sample_runs_opt_in_presentation_gate` 覆盖 `Examples/AdvancedVN/project.yaml`、`scenarios/advanced_presentation.yaml`、`vn.advanced_presentation_manifest`、release gate `vn.advanced_presentation`、`timeline.join_cancel`、`presentation.fallback`、`voice.sync`、`renderer.effect_budget`、save/load、replay hash、auto/skip/config state 和 player route |
| `S3-SYSTEM-01` | `DONE` | `cargo test -p astra-vn --test commercial_baseline`、`cargo test -p astra-vn --test system_controls`、`cargo test -p astra-release --test release_report release_gate_` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 `SystemStoryManifest` 必需入口检查、policy binding 阻断、`vn.system_story_manifest`/`vn.system_ui_profile_manifest` package section、`vn.system_ui_profile` release gate、save migration policy、gallery/replay unlock source policy、localization coverage、auto/skip/config/unlock state、缺 entry/profile manifest blocking diagnostic 和 `open_system: route_chart` player route slice |
| `S3-EDIT-01` | `DONE` | `cargo test -p astra-vn --test commercial_baseline` 覆盖 Graph node、Timeline track、wait/fence metadata 到 command id/source map 的校验、缺 command blocking diagnostic 和 patch manifest 只引用同一 IR command id |
| `S3-SAMPLE-01` | `DONE` | `cargo test -p astra-cli --test target_platform nativevn_sample_cooks_packages_validates_and_runs_full_playthrough` 使用 `Examples/NativeVN/project.yaml` 执行真实 `astra cook`、`astra package build`、`astra package validate` 和 `astra test run scenarios/full_playthrough.yaml --headless --package <built package>`，覆盖 sample full playthrough、scenario refs、save/load、replay hash、system state、`complete_wait` 和 `vn.commercial_baseline` release check |
| `S3-GAME-TARGET-01` | `IN_PROGRESS` | `cargo test -p astra-cli --test target_platform nativevn_sample_builds_windows_and_web_bundles_and_runs_player_routes`、`cargo test -p astra-release --test release_report release_gate_` 和 `cargo test -p astra-test --test vn_scenario` 覆盖 `nativevn-game` package gate、VN package section、scenario field、Windows/Web standalone bundle manifest、Windows entrypoint readiness、bundle 内 scenario refs、Windows `astra.player_route_report.v1` 和 Web browser host route report。该 evidence 只证明 bundle/package/route report slice，不证明 live player 由真实平台输入完整可玩 |
| `S3-PLAYER-AUTOMATION-01` | `IN_PROGRESS` | `cargo test -p astra-player-core`、`cargo test -p astra-player --test windows_input_automation`、`cargo test -p astra-player --test web_input_automation` 和 `cargo test -p astra-release release_gate_accepts_player_full_playable_only_with_matching_live_report` 覆盖 `astra.player_automation_script.v1`、`astra.player_input_transcript.v1`、`astra.player_automation_report.v1`、Windows `sendinput.*`/Web `cdp.*` transcript、视觉区域 hash 变化、音频 meter、route coverage、direct `route_scenario`/DOM click/JS callback/VN command 阻断，以及 `player.full_playable` 只在显式 live automation report 匹配 package hash/profile/target 时 pass；Stage 3 `DONE` 仍要等真实平台 host run 产出同 run window/browser evidence |
| `S3-TSUI-INTERNAL-DEMO-01` | `IN_PROGRESS` | `python Tools/TsuiNoSora/tests/test_asset_analysis.py`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k internal_demo_bundle`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k projectorrays`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k projectorrays_full_dump`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k full_resource_conversion`、`python Tools/TsuiNoSora/tests/test_asset_analysis.py -k nativevn_package_input`、`python Tools/check_docs.py`、`cargo test -p astra-cli --test target_platform tsuinosora_internal_demo_builds_asset_package_and_bundles`、`cargo test -p astra-player-core`、`cargo test -p astra-player --test windows_input_automation`、`cargo test -p astra-player --test web_input_automation` 和 `cargo test -p astra-release release_gate_accepts_player_full_playable_only_with_matching_live_report` 覆盖 `tsuinosora-internal-game` `classic` full-resource playable bundle pipeline shape：ProjectorRays 本地 dump adapter 脱敏、Director reader-required preflight 外部 reader evidence、`tsuinosora.projectorrays_full_dump_report.v1`、全量 binary chunk 转换覆盖 blocking、NativeVN `asset_roots`、asset sidecar、cooked asset package section、`asset.registry.assets`、同一 `.astrapkg` 派生 Windows/Web bundle manifest、`demo-config-template` repo-relative `.local` scaffold，以及 `player.full_playable` live report 校验。当前私有运行已生成 full dump coverage，但 2527 个 ProjectorRays binary chunk 的 converted resource evidence 仍为 0，`internal-demo-bundle` 返回 `TSUI_INTERNAL_DEMO_FULL_RESOURCE_CONVERSION_BLOCKED`；实际 internal playable bundle 尚未产出，不能标 `DONE`。`modern`、Patch-only、Runtime Patch/VFS 插件不属于本 milestone |
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

## 下一步实施顺序

| Order | Work | Status | Why now |
| --- | --- | --- | --- |
| 1 | `S2-PACKAGE-01` package container | `DONE` | `astra-package` 提供共享 container、Zstd codec、crypto descriptor、bounded reader；Runtime save 已迁移 |
| 2 | `S2-ASSET-01` + `S2-ASSET-02` asset/import/cook | `DONE` | `astra-asset` 和 `astra-cook` 提供 sidecar、registry、metadata import、DDC key 和 cook audit |
| 3 | `S2-GATE-01` release report | `DONE` | `astra-release` 和 `astra package validate` 输出 `astra.release_report.v1`；release profile 缺 `compiled.project` cook/project artifact 时阻断 |
| 4 | `S2-MEDIA-01` 到 `S2-MEDIA-05` media providers | `DONE` | `astra-media` 提供 headless renderer、TextLayout、AudioGraph、FilterGraph、DecodeProvider 和 optional native feature gates |
| 5 | `S2-WINDOWS-HOST-01` + `S2-WINDOWS-WMF-01` + `S2-WINDOWS-GATE-01` Windows platform repair | `DONE` | Windows host probe、WMF DecodeProvider 和 release gate evidence 已落地 |
| 6 | `S3-MODULE-LAYOUT-01` AstraVN module layout | `REOPENED_SPEC` | 将 `astra-vn` 从 Runtime 分区迁到 `Engine/Source/Modules/AstraVN/astra-vn`，并更新 workspace/path dependency |
| 7 | `S3-CRATE-SPLIT-01` AstraVN functional crate split | `REOPENED_SPEC` | 将单 crate 拆成多个功能 crate，`astra-vn` 只作为 facade、`rlib`/Rust ABI `dylib` 和兼容 re-export |
| 8 | `S3-DYLIB-01` AstraVN Rust dylib target | `REOPENED_SPEC` | 旧 facade smoke 和 VN cdylib provider fixture 已落地，但尚未证明 facade-only 多 crate 架构 |
| 9 | `S3-SCRIPT-01` + `S3-SCRIPT-02` `.astra` parser/compiler | `DONE` | `.astra` parser/compiler、grammar negative、source map、Story/Variable/Command/System manifest、route graph、stable hash 和 diagnostics 已落地 |
| 10 | `S3-ADVANCED-01` Advanced presentation opt-in profile | `DONE` | AdvancedVN sample、`vn.advanced_presentation_manifest`、release gate 和 headless scenario 已落地；普通 `classic`/`modern` profile 不被该 opt-in gate 阻断 |
| 11 | `S3-GAME-TARGET-01` NativeVN Game target | `IN_PROGRESS` | NativeVN sample cook/package/full playthrough、VN package sections、official policy source cache、standard command/presentation provider release gate、Scenario DSL `complete_wait` slice、Windows/Web standalone bundle 和 route report slice 已落地；仍需 `S3-PLAYER-AUTOMATION-01` 证明真实 player input/render/audio/output |
| 12 | `S3-PLAYER-AUTOMATION-01` Windows/Web live player automation | `IN_PROGRESS` | `astra-player-core`/`astra-player` 和 `player.full_playable` release check 已落地；下一步把真实 Windows window focus/`SendInput` 与 Chrome/Edge CDP host run 接到私有 acceptance，要求 input transcript、视觉 region hash 变化、音频 meter、host evidence 和 route evidence 同 run 产出 |
| 13 | `S3-TSUI-INTERNAL-DEMO-01` TsuiNoSora internal classic full-resource demo bundle | `IN_PROGRESS` | 本期只覆盖 `tsuinosora-internal-game` 的 `classic` profile，但完成条件是全量资源转换、原体验还原和 100% 可玩：ProjectorRays 本地 dump adapter、full dump coverage report、NativeVN asset sidecar、cooked asset package section、`asset.registry.assets`、同一 `.astrapkg` 派生 Windows/Web bundle 和 live automation report release check 入口已落地；当前阻断是 2527 个 ProjectorRays binary chunk 尚无 converted resource evidence。`modern`、Patch-only、Runtime Patch/VFS 插件移出本期 |
| 14 | `S3-TSUI-GATE-01` TsuiNoSora commercial validation gate | `IN_PROGRESS` | 脱敏 inventory、direct-readable extract preflight、Director `imap`/`mmap` resource map preflight、free mmap entry 计数、受限 `XFIR` RIFF/RIFX exact wrapper reader、opaque/compressed `XFIR` 与尾随未验证 bytes reader-required 阻断、Director `KEY*`/`CAS*` cast map preflight、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight、ProjectorRays 本地 dump adapter、`tsuinosora.script_source_map.v1` 脱敏 reader sidecar、reader id/hash/output contract evidence、script source map bytecode 阻断、route graph report、reference report、Asset analysis helper、native asset sidecar/cook/package registry、`stage3-gate`/`local-gate` orchestrator、NativeVN package input writer、project-level `package_sections`、package section release gate、公开 synthetic internal/patch player route gate、patch Web direct-read route check 和 Windows route-bound `mount_assets` local asset check 已落地；下一步接完整 Director/Shockwave cast parser/source-map reader、真实商业 full slice acceptance、正式 release signoff 和后续 patch/VFS 计划 |

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

| 15 | `S2-VFS-01` Asset VFS mount family | `REOPENED_SPEC` | 先把 `astra-asset` 升级为 VFS contract owner，补 package/local authorized/legacy pack/overlay mount、reader provider、resolve report 和 release gate；`astra-package` 只保留 package-backed mount source |
| 16 | `S3-RUNTIME-PROVIDER-01` NativeVN gameplay runtime provider | `REOPENED_SPEC` | 迁移已有 facade、VN extension manifest、package sections 和 release checks 到 `astra-vn-runtime-provider`，并保持 VN Core 权威语义 |
| 17 | `S4-PLUGIN-01` Plugin Manager UI | `REOPENED_SPEC` | Plugin Manager 需要显示和修改 Stage 1/2 的 enablement、dependency graph、VFS mount provider 和 gameplay runtime provider binding |
| 18 | `S4-EDITOR-RUNTIME-PROVIDER-01` Editor runtime provider switching | `REOPENED_SPEC` | Editor shell 需要读取 `RuntimeEditorMetadata`，切换 NativeVN surfaces，并把 provider/profile 传给 PIE 和 Release Gate |
| 19 | `S4-AI-01` 到 `S4-GATE-01` AI/MCP closure，包括 `S4-AI-ONNX` 和 `S4-AI-VFS-01` | `REOPENED_SPEC` | Runtime Director、provider profile、ONNX ModelBundle、Asset VFS/encryption 复用、memory、Context Pack、AI Control 和 release gate 需要一起落地 |
| 20 | `S4-EDITOR-TARGET-01` AstraEditor Editor target | `REOPENED_SPEC` | Editor target 需要 Qt/QML shell、PIE bridge 和 gameplay runtime provider selector |
| 21 | `S5-GAME-RUNTIME-01` + `S5-EMUCORE-SM-01` + `S5-LEGACY-VFS-01` | `REOPENED_SPEC` | AstraEMU 先接成 `AstraEmuRuntimeProvider`，再把 legacy VM 映射为 family-private scheduler/context/basic-block/action 状态机，并复用 Asset VFS legacy pack mount |
| 22 | `S5-MANAGER-01` + `S5-PROGRAM-TARGET-01` + `S5-FAMILY-01` + `S5-AUTOPROBE-01` + `S5-SCRIPT-01` + `S5-TEXT-01` + `S5-FILTER-01` | `REOPENED_SPEC` | AstraEMU Manager 仍作为 Program target，启动 gameplay runtime session、family plugin，并复用 Stage 4 provider、MCP 和 memory |
| 23 | `S6-LINUX-HOST-01` + `S6-MACOS-HOST-01` + `S6-IOS-HOST-01` + `S6-ANDROID-HOST-01` platform completion | `SPEC_READY` | Windows/Web 之外的平台完成从 Stage 2 移出，等 VN/Core/Editor 发布路径稳定后集中接入真实 SDK evidence |

## 验证命令

```bash
python Tools/check_docs.py
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected output: docs check reports checked markdown files；fmt/clippy/workspace tests pass；diff check has no whitespace errors。

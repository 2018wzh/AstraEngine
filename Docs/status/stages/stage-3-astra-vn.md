# Stage 3 AstraVN Work

Stage 3 把 EngineCore、Media 和 Package 组合成原生 VN 工作流。`.astra` 仍是 canonical story source；AstraVN Core 持有 VN 权威语义，Luau policy 只处理表现、系统页和复杂演出。AstraVN 已迁到 `Engine/Source/Modules/AstraVN/` 并拆成多个功能 crate；`astra-vn` 只保留 Rust ABI dylib facade 和兼容 re-export。`NativeVnRuntimeProvider` 已作为同级 gameplay runtime provider 接入，AstraVN 仍保持 VN 语义，不作为 AstraEMU 或 AstraRPG 的基类。

当前 Stage 3 是 `IN_PROGRESS`，不是 `DONE`。Migration 6 frontend 与 Migration 9 shared 1–3 已完成 focused implementation：`.astra` 使用 lossless token/CST/AST、固定 semantic passes、command registry、token source map、formatter 和 language-service adapter；共享 policy、component effects、versioned runtime envelope 与 async multi-session host 已落地。旗舰项目已直接替换原两路线 NativeVN 技术样例，并加入 180 条用户授权的中文发行配音。Stage 3 work item 不因此提前关闭：`S3-SCRIPT-01/02`、presentation、system UI、sample 和 Windows/Web Player 都必须等待同一 `.astrapkg` 的 formal native-input evidence。顶层还受 `S3-TSUI-INTERNAL-DEMO-01`、`S3-TSUI-GATE-01` 与 `S3-FLAGSHIP-DEMO-01` 阻断。

补充：`S3-TSUI-INTERNAL-DEMO-01` 的验收口径已提升为 full-resource classic playable bundle：`tsuinosora-internal-game` 必须完成全量 ProjectorRays dump coverage、全量资源转换 coverage、NativeVN `asset_roots`、asset sidecar、cooked asset package section、`asset.vfs_manifest`/`asset.catalog` package VFS evidence、同一 `.astrapkg` 派生 Windows/Web bundle manifest、`player.full_playable` live automation report 校验，以及原版/Demo 同 checkpoint 视觉截图对比。当前 repo-side pipeline 已落地 ProjectorRays 本地 dump adapter、脱敏 script source-map、Director reader-required preflight 外部 reader evidence、`demo-config-template`、带 `chunk_fourcc_counts`/`conversion_plan` 的 `tsuinosora.projectorrays_full_dump_report.v1`、`tsuinosora.projectorrays_converted_resources.v1` sidecar 校验、JSON-backed metadata chunk converter、`STXT` text converter、`Lscr` cast-member/source-number/CastScript/ParentScript source 映射和 malformed JSON numeric recovery、empty `Lscr` no-op metadata converter、`BITD` 1/16/32bpp PNG converter、8bpp `BITD` palette sidecar converter、KEY-bound `sndH`/`sndS` WAV converter、KEY-bound `ediM` `MACRZ` verified MP3 converter、score/metadata chunk 脱敏 converter、ProjectorRays `GO[...]` route identity 派生、ProjectorRays converted asset bridge、`tsuinosora.visual_screenshot_capture_report.v1`、`tsuinosora.visual_comparison_report.v1`、`capture_automation` 自动截图 intent/execution 脱敏记录、scenario refs package section、同 package Windows/Web bundle manifest、bundle 内原始分辨率 display config、Windows live window、`astra-player` Windows `SendInput` automation runner、player host consumed `TRACE` log 捕获、visual comparison hash 绑定、host conformance callback meter 和 release `player.full_playable` identity continuity 校验。私有 acceptance 命令是 `python Tools/TsuiNoSora/tsuinosora_tools.py internal-demo-bundle --config Examples/TsuiNoSora/.local/demo.config.json --repo-root .`；当前私有 dump 覆盖 2527 个 ProjectorRays binary chunk，2527 个 chunk 均有 converted evidence，`demo-slice` 可生成 28 条脱敏 route 的 NativeVN project/package input，`internal-demo-bundle` 已从同一个 `.astrapkg` 产出 Windows/Web bundle manifest，并能采集原版/Demo 同 checkpoint 截图且通过 title 视觉 comparison；最近一次 Windows live automation 中 28 次 `SendInput` 都被 player host consumed trace 证明。验收仍因 Windows live player 在真实输入后没有产生可见状态变化、automation transcript 未覆盖 28 条 full classic route、缺 required manual signoff 而 blocking，不能作为独立 milestone 标 `DONE`。`modern`、Patch-only 和 Runtime Patch/VFS 插件仍不属于该 milestone，但 full-resource conversion、原体验还原和 100% 可玩是该 milestone 的完成条件。

补充：Windows/Web route report 只证明 bundle host 能读取 package、scenario refs、route model 和 mount policy 并输出脱敏 route evidence；它不能证明完整可视游玩。Stage 3 `DONE` 必须等待 `player.full_playable` 通过平台原生输入自动化闭合，证据包括 input transcript、player host consumed trace、视觉区域变化、视觉截图对比、音频 meter、平台 host evidence 和同次 route evidence。当前 `astra-player-core`、`astra-player` 和 `astra-release` 已能阻断 direct `route_scenario`、DOM click、JS callback 和直接 VN command 冒充玩家输入；Windows private acceptance 已能生成 live automation report，并用 player host `TRACE` log 证明 `SendInput` 被窗口消费，但该 report 会在视觉区域未变化或 full route coverage 缺失时保持 blocking。

补充：视觉截图验收只在 ignored `.local` 保存原版/Demo 截图，repo-side report 只记录 checkpoint id、route id、region id、尺寸、hash、差异指标、自动采集执行摘要、视觉 review hash 和 diagnostic。缺原版截图、缺 Demo 截图、same-run capture role 缺失、blank frame、尺寸错误、关键区域差异过大、缺 required 视觉 review、review fail 或自动采集执行失败都会让 `internal-demo-bundle` blocking。

补充：Director Lingo preflight 现在还记录 `Lctx` entry count/table hash；`Lctx` 与 `Lnam` 都不输出原始 table payload。Malformed `Lctx` table 或未终止 `Lnam` table 会 blocking。

补充：Director cast preflight 现在会阻断重复 `CASt` binding；同一个 `CASt` 被多个 `CAS*` library/slot 绑定时不能进入 cast/source-map evidence。

补充：Cast source-map report 现在会阻断手写 `tsuinosora.cast_map.v1` 或外部 `tsuinosora.director_cast_map.v1` sidecar 中的 payload/正文/bytecode 字段，diagnostic 只记录字段路径。

补充：TsuiNoSora release gate 现在会在读取 package section 时统一阻断 payload-like 字段泄露；`script_text`、`source_text`、`content`、`payload`、`bytecode` 等字段不能进入 `tsuinosora.reference_evidence`、`asset_analysis`、`conversion_manifest`、`mount_policy`、`modern_profile_report` 或 `manual_signoff` section，`redaction.payload: omitted` 除外。

补充：TsuiNoSora release gate 现在还会阻断空 `tsuinosora.asset_analysis`；`status: pass` 但没有任何 analyzed asset 不能证明 Asset Analysis Gate 已经完成。

补充：TsuiNoSora release gate 现在还会阻断空 `tsuinosora.conversion_manifest.resources`；即使所有 routes 都是 `covered`，没有 converted resource evidence 也不能证明真实转换完成。每条 resource 还必须包含 source/native 相对路径、classification、source hash、converted hash 和正 byte size。

补充：Director cast preflight 现在能读取显式 `tsuinosora.director_cast_member_metadata.v1`，只保留 kind、route id、command id、anchor、bounds 和 metadata hash，并把这些脱敏字段继续传入 `tsuinosora.cast_source_map_report.v1`；普通 `CASt` payload 仍只记录 hash。

补充：`tsuinosora.director_cast_member_metadata.v1` 的 anchor/bounds 会做数值校验；anchor 非数值或 bounds 负尺寸会 blocking，防止 route/layout evidence 被静默丢弃。

补充：Director cast metadata 的 `kind: character_atlas` 必须携带 parts；part id、pose、expression、anchor、crop、layer、mouth/eye state compatibility 和 fallback 会进入 Director cast report，并继续传入 `tsuinosora.cast_source_map_report.v1`。

补充：`tsuinosora.visual_reference_report.v1` 现在会校验默认 `Title.png`/`Game.png` 的固定尺寸和 hash；缺文件、PNG 不可读、hash mismatch 或 dimensions mismatch 会阻断 Stage 3 gate，release gate 也会阻断固定 hash/dimensions mismatch，report 仍只记录 hash、尺寸、区域 id 和 diagnostic。

补充：Route graph 和 script source-map report 现在会阻断重复 `route_id` 冲突；同一 `route_id` 指向不同 terminal/choice signature 时不能进入 NativeVN package input。

补充：Route graph 和 script source-map report 现在也会阻断同一 route 内重复 choice id，避免重复 choice 进入 `.astra` option key 或 scenario `player_input choose`。

补充：`stage3-gate` 现在只在 route graph 缺失时使用 script source-map fallback；如果 route graph sidecar 存在但 payload、symbol、coverage 或 duplicate 检查失败，fallback 不能让 Stage 3 通过。

补充：`tsuinosora.script_source_map.v1` 覆盖 unsupported `Lscr` bytecode 时，route 必须同时绑定 `director_lingo_map.json` source/hash、`script_resource_id` 和 `script_payload_sha256`；缺失、未知 resource 或 hash mismatch 都会让 `tsuinosora.script_source_map_report.v1` blocking。

补充：同一个 `director_lingo_map.json` 中的 unsupported `Lscr` 必须逐个 resource 覆盖；只覆盖部分 `script_resource_id`/`script_payload_sha256` 时，`tsuinosora.script_source_map_report.v1` 仍会 blocking。

补充：`local-gate` 现在要求 routes 来自 `stage3-gate` 派生的 route graph 或 script source-map report；显式 routes 输入会触发 `TSUI_LOCAL_GATE_ROUTE_EVIDENCE_REQUIRED`，不能写 NativeVN package input。

Windows patch route 的 `player.patch_direct_read` 不能只靠 `AstraPlayer.mount_policy.json` 通过；缺少 `mount_probes` 或 route-bound `mount_assets` 本地读取证据时必须 blocking。

TsuiNoSora conversion slice 还要求 Asset analysis 通过后真实写入 `local_work_root/native-assets/`，并通过 `tsuinosora.native_asset_rearrange_report.v1` 与 `tsuinosora.conversion_report.v1` 记录 source/native 相对路径、classification、source hash、converted hash、byte size 和 converted/missing asset count；该 report blocked 时 conversion blocked。

NativeVN package input 会把 route graph/source map 的 sanitized choice id 保留到 `.astra` option key 和 scenario `player_input choose`，多 choice route 按 source-map 顺序生成连续 choice state；缺 choice 证据时才使用 fallback choice。

补充：NativeVN package input 现在会重新校验显式 route 输入；不安全 `route_id`/terminal/choice、非 covered coverage、重复 choice 或冲突 route signature 会 blocking，并且不会写出 story 或 scenario refs。

`stage3-gate` 派生的 route-bound cast member 会通过 `tsuinosora.cast_source_map_report.v1` 的 source hash 和 `tsuinosora.native_asset_rearrange_report.v1` 的 converted hash 写成 `mount_assets`，并随 conversion report 进入 `local-gate` 生成的 patch/windows scenario refs；没有显式 routes 入参时也不能丢失 choice 或 mount evidence。

当前 evidence：

```bash
cargo test -p astra-vn-script
cargo test -p astra-vn-script --test compiler_diagnostics
cargo test -p astra-vn --test vn_dylib_facade
cargo test -p astra-vn-plugin --test vn_plugin_extensions
cargo test -p astra-vn-package --test commercial_baseline
cargo test -p astra-vn-policy --test luau_mutation
cargo test -p astra-vn-policy --test policy_bundle
cargo test -p astra-vn-presentation --test presentation_model
cargo test -p astra-vn-presentation --test presentation_execution
cargo test -p astra-vn-package --test advanced_presentation
cargo test -p astra-media --test filter_graph
cargo test -p astra-vn-system --test system_controls
cargo test -p astra-vn-runtime-provider --test game_runtime_provider
cargo test -p astra-vn-runtime-provider --test runtime_provider_ffi
cargo test -p astra-runtime --test trigger_event
cargo test -p astra-test --test vn_scenario
cargo test -p astra-cli --test target_platform nativevn_minimal_profile_cooks_packages_and_runs_headless -- --exact
cargo test -p astra-player-core
cargo test -p astra-player --features platform-test-driver --test windows_input_automation
cargo test -p astra-player --test web_input_automation
cargo test -p astra-release release_gate_accepts_player_full_playable_only_with_matching_live_report
cargo test -p astra-release --test release_report release_gate_
python Tools/TsuiNoSora/tests/test_asset_analysis.py
python Tools/TsuiNoSora/tests/test_asset_analysis.py -k internal_demo_bundle
python Tools/TsuiNoSora/tests/test_asset_analysis.py -k projectorrays
python Tools/check_docs.py
```

## S3-MODULE-LAYOUT-01 AstraVN module layout

**ID:** `S3-MODULE-LAYOUT-01`

**Status:** `DONE`

**Goal:** 将现有 `astra-vn` 从 Runtime 分区迁到 `Engine/Source/Modules/AstraVN/astra-vn`，保持 package name、public API 和现有测试入口兼容。

**Depends On:** `S3-DYLIB-01`、[AstraVN Module Layout Migration](../../migrations/astra-vn-module-layout-migration.md)

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn/`、`Cargo.toml`、`Engine/Source/Developer/astra-test/Cargo.toml`、`Engine/Source/Developer/astra-release/Cargo.toml`、`Engine/Source/Programs/astra-cli/Cargo.toml`

**Steps:**

1. 移动 `astra-vn` crate 到 `Engine/Source/Modules/AstraVN/astra-vn`。
2. 更新 root workspace member 和下游 `astra-vn` path dependency。
3. 更新 `astra-vn` 内部对 Runtime、Platform 和 Plugin crate 的相对 path。
4. 清理实现计划、workspace blueprint、coverage matrix 和 stage test matrix 中的旧目标路径。
5. 编写路径泄漏检查，确保旧路径只在 migration 文档中作为迁移前路径出现。

**Done Evidence:** `cargo metadata --no-deps`、`cargo test -p astra-vn --test vn_dylib_facade`、`cargo test -p astra-test --test vn_scenario` 和 `python Tools/check_docs.py` 通过；旧 Runtime 路径不再作为目标实现路径出现。

**Linked Test IDs:** `T-S3-MODULE-LAYOUT-01`

## S3-CRATE-SPLIT-01 AstraVN functional crate split

**ID:** `S3-CRATE-SPLIT-01`

**Status:** `DONE`

**Goal:** 将现有单 crate `astra-vn` 拆成 AstraVN 多功能 crate，并把 `astra-vn` 收缩为 facade、`rlib`/Rust ABI `dylib` 和兼容 re-export。

**Depends On:** `S3-MODULE-LAYOUT-01`、[AstraVN Crate Split Migration](../../migrations/astra-vn-crate-split-migration.md)

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-script/`、`Engine/Source/Modules/AstraVN/astra-vn-core/`、`Engine/Source/Modules/AstraVN/astra-vn-policy/`、`Engine/Source/Modules/AstraVN/astra-vn-presentation/`、`Engine/Source/Modules/AstraVN/astra-vn-commands/`、`Engine/Source/Modules/AstraVN/astra-vn-system/`、`Engine/Source/Modules/AstraVN/astra-vn-save/`、`Engine/Source/Modules/AstraVN/astra-vn-package/`、`Engine/Source/Modules/AstraVN/astra-vn-plugin/`、`Engine/Source/Modules/AstraVN/astra-vn-editor/`、`Engine/Source/Modules/AstraVN/astra-vn-runtime-provider/`、`Engine/Source/Modules/AstraVN/astra-vn/`

**Steps:**

1. 建立 sibling crate skeleton，并加入 root workspace。
2. 按 script、presentation、core、policy、commands、system、save、package、plugin、editor、runtime-provider、facade 顺序搬迁现有模块。
3. 让 `astra-vn` facade 只保留 `pub use astra_vn_*::*;`、facade 文档和 dylib smoke。
4. 确认功能 crate 不依赖 `astra-vn` facade；共享 DTO 下沉到更底层 crate。
5. 保持现有 `astra_vn::*` 消费路径先通过 facade re-export 兼容。

**Done Evidence:** `cargo metadata --no-deps`、各 `astra-vn-*` crate 测试、`cargo test -p astra-vn --test vn_dylib_facade`、`cargo test -p astra-vn-plugin --test vn_plugin_extensions`、`cargo test -p astra-release --test release_report release_gate_` 和 `python Tools/check_docs.py` 通过；`astra-vn` 不再承载 parser/runtime/policy/package 业务实现。

**Linked Test IDs:** `T-S3-CRATE-SPLIT-01`

## S3-DYLIB-01 AstraVN Rust dylib target

**ID:** `S3-DYLIB-01`

**Status:** `DONE`

**Goal:** `astra-vn` 作为 facade crate，同时产出 `rlib` 和 Rust ABI `dylib`，re-export AstraVN 子 crate public API，并证明 facade-only 架构仍兼容现有 consumer。

**Depends On:** `S1-DYLIB-01`、`S3-CRATE-SPLIT-01`、`S3-SCRIPT-02`、`S3-CORE-03`、`S3-PLUGIN-01`、`S3-PRESENT-01`、`S3-SYSTEM-01`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn/Cargo.toml`、`Engine/Source/Modules/AstraVN/astra-vn/src/lib.rs`、`Engine/Source/Modules/AstraVN/astra-vn/tests/vn_dylib_facade.rs`、`Engine/Source/Modules/AstraVN/astra-vn-plugin/tests/vn_plugin_extensions.rs`、`Engine/Plugins/Fixtures/vn-extension-provider`

**Steps:**

1. 在 `astra-vn` crate 中声明 `crate-type = ["rlib", "dylib"]`，并保留普通 workspace `rlib` 使用路径。
2. 让 facade re-export `astra-vn-script`、`astra-vn-core`、`astra-vn-policy`、`astra-vn-presentation`、`astra-vn-commands`、`astra-vn-system`、`astra-vn-save`、`astra-vn-package`、`astra-vn-plugin`、`astra-vn-editor` 和 `astra-vn-runtime-provider` public API，不复制 EngineCore runtime 或业务实现。
3. 约束 dylib 只用于同 engine version、rustc fingerprint 和 feature fingerprint 下的 Rust-side 动态链接；外部稳定边界仍是 `.astra`、package section 和 Stage 1 plugin ABI。
4. 禁止通过 dylib public API 暴露 Luau VM handle、renderer/audio native handle、Actor 指针或 Editor widget。
5. 编写 facade smoke，证明 `astra-vn` public API 能创建 VN runtime state、读取 command manifest、登记 VN extension DTO，并与 `astra-engine` facade 共同链接。
6. 编写真实 cdylib provider fixture，证明 VN extension provider 经过 Stage 1 plugin ABI build/load/unload 后可进入 provider registry，并生成可校验 `VnExtensionManifest`。

**Done Evidence:** `cargo test -p astra-vn --test vn_dylib_facade` 和 `cargo test -p astra-vn-plugin --test vn_plugin_extensions` 通过；`Cargo.toml` 明确产出 `rlib` 与 `dylib`，`astra-vn` 只做 facade re-export，`vn-extension-provider` 真实 cdylib fixture 通过 `PluginLoader` 加载、注册 VN policy/command/presentation/editor metadata/release check provider slots，并在 unload 后释放注册项。

**Linked Test IDs:** `T-S3-DYLIB-01`

## S3-RUNTIME-PROVIDER-01 NativeVN gameplay runtime provider

**ID:** `S3-RUNTIME-PROVIDER-01`

**Status:** `DONE`

**Goal:** 在 `astra-vn-runtime-provider` 中组合 AstraVN 子 crate，把现有 facade、VN Core、Luau policy、VN extension manifest、package sections 和 release checks 包装为 `NativeVnRuntimeProvider`。

**Depends On:** `S3-CRATE-SPLIT-01`、`S3-DYLIB-01`、`S3-CORE-03`、`S3-PLUGIN-01`、[Game Runtime Provider Contract](../../contracts/game-runtime-provider.md)、[Game Runtime Provider Blueprint](../../implementation/game-runtime-provider.md)

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-runtime-provider/src/lib.rs`、`Engine/Source/Modules/AstraVN/astra-vn-runtime-provider/tests/game_runtime_provider.rs`、`Engine/Source/Modules/AstraVN/astra-vn-runtime-provider/tests/runtime_provider_ffi.rs`、`Engine/Source/Runtime/astra-plugin-abi/src/lib.rs`、`Engine/Source/Developer/astra-test/src/runner.rs`、`Engine/Source/Developer/astra-release/src/lib.rs`

**Steps:**

1. 定义 `NativeVnRuntimeProvider` descriptor、prepare/probe/open/step/save/restore/shutdown、package section plan、release checks 和 editor metadata。
2. 让 project target 显式绑定 `native_vn` runtime provider，不按插件加载顺序选择玩法 runtime。
3. 把现有 VN command、presentation command、Luau policy bundle、Graph/Timeline metadata 和 release check provider binding 挂到 gameplay runtime provider selection。
4. 保持 `VnRuntimeState`、`vn.runtime_state`、`vn.policy_state`、VN package sections 和 `vn.*` release gate 的兼容性。
5. 编写 provider smoke、package/release continuity、missing binding blocking 和 replay hash 测试。

**Done Evidence:** `cargo test -p astra-plugin-abi runtime_provider_abi`、`cargo test -p astra-plugin runtime_provider_registry`、`cargo test -p astra-vn-runtime-provider --test game_runtime_provider`、`cargo test -p astra-vn-runtime-provider --test runtime_provider_ffi`、`cargo test -p astra-test --test vn_scenario`、`cargo test -p astra-release --test release_report runtime_provider` 和 `cargo test -p astra-cli --test target_platform nativevn_minimal_profile_cooks_packages_and_runs_headless` 通过。NativeVN session 使用 RuntimeWorld 的 `astra.vn.step` action；FFI 覆盖 instance create/destroy、package-bound open、step、hashed save section、restore、shutdown 和活动 session destroy blocker；release report 输出 behavior state/event/presentation hash。VN provider 不能被 AstraEMU/AstraRPG 当作基类复用。

**Linked Test IDs:** `T-S3-RUNTIME-PROVIDER-01`

## S3-SCRIPT-01 `.astra` parser

**ID:** `S3-SCRIPT-01`

**Status:** `IN_PROGRESS`

**Goal:** 把现有 line parser 迁到标准 frontend parser，覆盖 `.astra` 缩进块、story、state、scene、stage、text、choice、call/return、command id、lossless trivia 和 token/attribute span。

**Depends On:** `Docs/modules/astra-vn-script.md`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-script/src/parser.rs`、`Engine/Source/Modules/AstraVN/astra-vn-script/src/compiler.rs`、`Engine/Source/Modules/AstraVN/astra-vn-script/tests/compiler_runtime.rs`、`Engine/Source/Modules/AstraVN/astra-vn-script/tests/compiler_diagnostics.rs`、`Engine/Source/Modules/AstraVN/astra-vn-package/tests/commercial_baseline.rs`

**Steps:**

1. 保留 `compile_astra_sources` 兼容入口，新增标准 Lexer、TokenStream、Lossless CST 和 Typed AST adapter。
2. 支持 command id、text key、speaker、voice、choice option、jump target、comment、blank line 和 source id token span。
3. 保留 source map 所需的 byte range、line/column、attribute span 和 expanded command id。
4. 编写有效 sample、quote/arrow/indent/source id/duplicate attr/orphan option/缺 key/未知 system page、重复 command id、重复 source id、缺失 jump/choice target、trivia round-trip 和诊断定位测试。

**Baseline Evidence:** `cargo test -p astra-vn-script --test compiler_runtime`、`cargo test -p astra-vn-script --test compiler_diagnostics` 和 `cargo test -p astra-vn-package --test commercial_baseline` 通过。现有 parser 能解析 `.astra` story/state/scene/text/choice/system page option/jump/call/return/mutate；缩进参与 canonical story/state/scene/command/choice option 归属，option 只能紧邻 choice，system page option 走独立规则；quote、arrow、结构缩进、empty source id、duplicate attr、orphan/detached option、缺 key、未知 system page、重复 id/text key、非法变量域/数字、scene 外 command、缺失 target 和 unreachable main state 都输出 blocking diagnostic 与 source span。该 evidence 仍不关闭 Lossless CST、Typed AST、token span、formatter/LSP 等 frontend 标准化工作。

**Linked Test IDs:** `T-S3-SCRIPT-01`

## S3-SCRIPT-02 Compiler 到 CompiledStory IR

**ID:** `S3-SCRIPT-02`

**Status:** `IN_PROGRESS`

**Goal:** 用显式 semantic passes 从 Typed AST lowering 到 `CompiledStory` IR、StoryManifest、VariableManifest、CommandManifest、SystemStoryManifest、SourceMap、DebugSymbols 和 release conformance evidence。

**Depends On:** `S3-SCRIPT-01`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-script/src/compiler.rs`、`Engine/Source/Modules/AstraVN/astra-vn-script/src/types.rs`、`Engine/Source/Modules/AstraVN/astra-vn-script/tests/compiler_runtime.rs`、`Engine/Source/Modules/AstraVN/astra-vn-script/tests/compiler_diagnostics.rs`

**Steps:**

1. 以当前 `CompiledStory` Rust schema 为 baseline，文档中的 `luau_manifest`、`timeline_ir`、`text_effect_ir`、token span 和 command source map 先作为 migration target。
2. 拆分 `lower::symbols`、`lower::routes`、`lower::variables`、`lower::commands`、`lower::system_stories` 和 `lower::compiled_story`。
3. 通过 `CommandRegistry` 校验 Core、standard presentation 和 extension command，release profile 遇到 unknown command 必须 blocking。
4. 编写 IR snapshot、semantic pass equivalence、source map lookup、command registry、formatter semantic hash、LSP diagnostic adapter 和 reachability diagnostic 测试。

**Baseline Evidence:** `cargo test -p astra-vn-script --test compiler_runtime` 和 `cargo test -p astra-vn-script --test compiler_diagnostics` 通过；当前 `CompiledStory` 直接包含 Story/Variable/Command/System manifest、route graph、source map、debug symbols 和 stable hash，package section 使用 compiler 输出中的 `system_story_manifest`，diagnostic 可定位源文件。该 evidence 只证明当前 compiler baseline，不能关闭 semantic pass、command registry 和 source map 升级。

**Linked Test IDs:** `T-S3-SCRIPT-02`

## S3-CORE-01 VN Core dialogue、choice 与变量域

**ID:** `S3-CORE-01`

**Goal:** AstraVN Core 实现 dialogue、choice、variables、call/return 和 route flags 的权威语义。

**Depends On:** `S1-RUNTIME-02`、`S3-SCRIPT-02`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-core/src/runtime.rs`、`Engine/Source/Modules/AstraVN/astra-vn-core/src/types.rs`、`Engine/Source/Modules/AstraVN/astra-vn-core/tests/compiler_runtime.rs`、`Engine/Source/Modules/AstraVN/astra-vn-package/tests/commercial_baseline.rs`、`Engine/Source/Developer/astra-test/tests/vn_scenario.rs`

**Steps:**

1. 按 [AstraVN StateMachine Playback](../../implementation/astra-vn-state-machine.md) 把 CompiledStory command 驱动为 Runtime `StateMachine` action。
2. 实现 project、global、temp、system 四个变量域和写入规则。
3. 实现 `VnRuntimeState`、`VnCommandCursor`、dialogue wait、choice wait、call/return stack 和 route flag。
4. 编写 command cursor、dialogue advance、choice payload、variable rollback 和 call/return 测试。

**Done Evidence:** `cargo test -p astra-vn-core`、`cargo test -p astra-vn-runtime-provider --test game_runtime_provider`、`cargo test -p astra-vn-package --test commercial_baseline`、`cargo test -p astra-vn-system --test system_controls`、`cargo test -p astra-runtime --test trigger_event` 和 `cargo test -p astra-test --test vn_scenario` 证明结构化 command cursor、dialogue/choice/system wait、Runtime AwaitToken、variables、call/system stack、skip-read、route coverage/flags、audio/timeline effect、mutation 和 `astra.vn.step` trigger/action trace 不依赖 Luau policy。

**Linked Test IDs:** `T-S3-CORE-01`

## S3-CORE-02 Backlog、read-state 与 voice replay

**ID:** `S3-CORE-02`

**Goal:** Backlog、read-state 和 voice replay 由 AstraVN Core 统一维护。

**Depends On:** `S3-CORE-01`、`S2-MEDIA-02`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-core/src/runtime.rs`、`Engine/Source/Modules/AstraVN/astra-vn-core/tests/compiler_runtime.rs`、`Engine/Source/Modules/AstraVN/astra-vn-system/tests/system_controls.rs`、`Engine/Source/Modules/AstraVN/astra-vn-save/tests/vn_save_container.rs`

**Steps:**

1. 定义 BacklogEntry，保存 command id、text key、speaker、voice ref、layout metadata、read flag 和 route position。
2. 实现 read-state mark、skip eligibility 和 voice replay lookup。
3. 确认 Luau policy 只能请求展示，不能改写 Core backlog/read-state。
4. 编写 backlog append、skip read-only、voice replay available 和 replay hash 测试。

**Done Evidence:** `cargo test -p astra-vn-core --test compiler_runtime`、`cargo test -p astra-vn-system --test system_controls` 和 `cargo test -p astra-vn-save --test vn_save_container` 通过；Core 维护 rich backlog、read-state 和 voice replay，skip-read 只跳过已读 dialogue，`VnReplayUiState` 输出 replay UI snapshot/hash，并随 save/load 保持一致。

**Linked Test IDs:** `T-S3-CORE-02`

## S3-CORE-03 VN save/load/replay integration

**ID:** `S3-CORE-03`

**Goal:** VN 状态接入 Stage 1 Save/Replay，覆盖 route、变量、backlog、read-state、voice replay 和 Luau snapshot ref。

**Depends On:** `S1-SAVE-01`、`S3-CORE-02`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-save/src/lib.rs`、`Engine/Source/Modules/AstraVN/astra-vn-save/tests/vn_save_container.rs`

**Steps:**

1. 定义 VN save section，包含 route state、command cursor、variables、backlog、read-state 和 voice replay index。
2. 接入 Runtime replay hash，输出 VN command 维度 mismatch。
3. 处理 Luau policy snapshot ref，但不保存 function、thread、userdata 或 native handle。
4. 编写 save-load-resume、replay-from-start 和 invalid snapshot 测试。

**Done Evidence:** `cargo test -p astra-vn-runtime-provider --tests`、`cargo test -p astra-plugin --test product_runtime_host` 和 `cargo test -p astra-test --test vn_scenario` 通过；VN state 与 Luau policy state 作为 typed component 进入完整 RuntimeSnapshot，并由唯一 `runtime.world`/`astra.runtime.save_blob.v2` section 承载。证据覆盖 outer/nested hash、损坏回滚、完整 queue/mutation/effect trace、restored step/seed、restore continuation、delta/seed/mode drift 和 live-provider replay 阻断；拆分 state blob 不再作为产品权威 save。

**Linked Test IDs:** `T-S3-CORE-03`

## S3-LUAU-01 Luau sandbox 与 Mutation API

**ID:** `S3-LUAU-01`

**Goal:** Luau 通过 `mlua` 进入策略层，默认无文件、网络或系统调用，权威写入必须走 `astra.mutate`。

**Depends On:** `S3-CORE-01`、`Docs/contracts/script-vn.md`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-policy/src/luau.rs`、`Engine/Source/Modules/AstraVN/astra-vn-policy/tests/luau_sandbox.rs`、`Engine/Source/Modules/AstraVN/astra-vn-policy/tests/luau_mutation.rs`

**Steps:**

1. 建立 Luau runtime sandbox，默认禁用 fs、network 和系统调用。
2. 实现 `astra.command`、`astra.mutate`、`astra.var`、`astra.query` 和 `astra.trace` public API。
3. 记录每次 mutation 的 trace、previous value、rollback scope 和 replay event，并支持 rollback/playback。
4. 拒绝不可序列化 snapshot、command payload 和 trace fields。
5. 编写 sandbox denied、mutation recorded、rollback/playback、command/query/trace recorded、direct table write ignored 和 payload blocked 测试。

**Done Evidence:** `cargo test -p astra-vn-policy --test luau_sandbox` 和 `cargo test -p astra-vn-policy --test luau_mutation` 通过；`mlua` sandbox 禁用 fs/network/module escape，text/asset/backlog/savepoint/layout query 读取注入的 `PolicyQueryContext` 并记录 result hash，缺 backing 不返回合成值；interrupt、memory、output 和 snapshot depth budget 会阻断失控执行；`astra.var.set` authority bypass 被禁用。mutation/command/query/trace/snapshot 只产生可序列化 state/trace，rollback/playback 可恢复或重放 policy state。

**Linked Test IDs:** `T-S3-LUAU-01`

## S3-LUAU-02 官方 policy bundle 与 system stories

**ID:** `S3-LUAU-02`

**Goal:** 提供官方标准 policy bundle，覆盖 message UI、choice UI、title、config、gallery、replay 和 chart system stories。

**Depends On:** `S3-LUAU-01`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-policy/src/policy_bundle.rs`、`Engine/Source/Modules/AstraVN/astra-vn-policy/src/standard_policy.luau`、`Engine/Source/Modules/AstraVN/astra-vn-policy/tests/policy_bundle.rs`、`Engine/Source/Modules/AstraVN/astra-vn-package/tests/commercial_baseline.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs`

**Steps:**

1. 定义 `astra.policy_bundle.v1` manifest、Luau entry、capabilities、dependencies、package lock、source hash 和 source cache section。
2. 实现官方 `astra.policy.standard` source，覆盖 message、choice、system page 和 timeline command registration。
3. 让 policy command 提供 schema、role、performance budget、trace 和 snapshot evidence，包内只记录 source hash、byte size 和相对 entry。
4. 编写 source cache hash/size、缺 cache、hash mismatch、system story reachability、missing entry 和 policy lock 测试。

**Done Evidence:** `cargo test -p astra-vn-policy --test policy_bundle` 和 `cargo test -p astra-release --test release_report release_gate_` 通过；标准 policy bundle 随 package 写入 `vn.policy_bundle_manifest` 和 `vn.policy_bundle_source_cache`，release gate 会校验 required capabilities、lock hash、source hash、byte size、source cache section 和缺失/篡改阻断，不在 report 输出 Luau source payload。

**Linked Test IDs:** `T-S3-LUAU-02`

## S3-PLUGIN-01 VN plugin extension points

**ID:** `S3-PLUGIN-01`

**Goal:** AstraVN 把 Luau policy bundle provider、VN command provider、presentation command provider、Graph/Timeline metadata extension 和 VN release check 接入 Stage 1 extension registry。

**Depends On:** `S1-PLUGIN-03`、`S3-LUAU-02`、`S3-PRESENT-01`、`S3-EDIT-01`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-plugin/src/lib.rs`、`Engine/Source/Modules/AstraVN/astra-vn-plugin/tests/vn_plugin_extensions.rs`

**Steps:**

1. 定义 Luau policy bundle provider、VN command provider 和 presentation command provider 的 extension point。
2. 让 Graph node、timeline track 和 Inspector metadata 只保存 authoring metadata，不产生第二套 runtime model。
3. 把 provider binding 写入 project manifest，并随 package 进入 `plugin.extension_registry`。
4. 编写 missing policy provider、command provider conflict、metadata extension roundtrip 和 release check binding 测试。

**Done Evidence:** `cargo test -p astra-vn-plugin --test vn_plugin_extensions` 和 `cargo test -p astra-release --test release_report release_gate_` 通过；VN 插件扩展由 Stage 1 registry 和 Stage 2 gate 校验，Luau policy bundle、VN command、presentation command、Graph/Timeline metadata 和 release check provider 必须显式绑定，加载顺序不能改变 VN runtime 行为。

**Linked Test IDs:** `T-S3-PLUGIN-01`

## S3-PRESENT-01 Presentation model 与标准命令库

**ID:** `S3-PRESENT-01`

**Status:** `IN_PROGRESS`

**Goal:** 实现 `StageModel`、`LayerState`、`CameraState`、`TextWindowState`、`VideoLayerState`、`PresentationTimeline` 和标准命令库。

**Depends On:** `S3-SCRIPT-02`、`S2-MEDIA-04`、[AstraVN Presentation Model](../../modules/astra-vn-presentation-model.md)、[AstraVN Standard Command Library](../../modules/astra-vn-standard-commands.md)

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-presentation/src/presentation.rs`、`Engine/Source/Modules/AstraVN/astra-vn-presentation/src/presentation_execution.rs`、`Engine/Source/Modules/AstraVN/astra-vn-package/tests/commercial_baseline.rs`、`Engine/Source/Modules/AstraVN/astra-vn-presentation/tests/presentation_model.rs`、`Engine/Source/Modules/AstraVN/astra-vn-presentation/tests/presentation_execution.rs`

**Steps:**

1. 定义 Stage/Layer/Camera/Sprite/TextWindow/VideoLayer serde 类型和 schema。
2. 实现 `show`、`hide`、`move`、`camera`、`transition`、`shake`、`movie`、`voice`、`bgm`、`se`、`wait`、`choice`、`system_page` 的 schema、IR 和 release check；当前已有 `VnStandardCommandManifest`、usage validation 和 release gate slice。
3. 实现 skip、auto、replay、voice sync、movie end、fallback、`VnWaitState` 映射和 performance budget；当前已有 serializable `VnWaitState`、movie/voice/timeline wait capability 和 real player `complete_wait` slice。
4. 编写 provider binding、timeline join/cancel、voice fence、effect fallback 和 deterministic hash 测试；当前已有 `VnPresentationProviderManifest`、filter fallback policy gate、`CpuFilterExecutor`、`VnHeadlessPresentationExecutor`、`VnAdvancedPresentationManifest` 和 advanced presentation scenario evidence。

**Done Evidence:** `cargo test -p astra-media --test filter_graph`、`cargo test -p astra-vn-presentation --test presentation_execution`、`cargo test -p astra-vn-presentation --test presentation_model`、`cargo test -p astra-vn-package --test advanced_presentation`、`cargo test -p astra-vn-presentation --test await_gates`、`cargo test -p astra-vn-commands --test standard_command_manifest`、`cargo test -p astra-release --test release_report release_gate_` 和 `cargo test -p astra-test --test vn_scenario` 通过；标准命令从 `.astra` 编译到 IR，headless Runtime 输出稳定 PresentationCommand/AudioCommand、Timeline task、FilterGraph CPU execution、fallback policy、movie/voice/timeline wait 和 serializable `VnWaitState`。

**Linked Test IDs:** `T-S3-PRESENT-01`

## S3-SYSTEM-01 System UI profile

**ID:** `S3-SYSTEM-01`

**Status:** `IN_PROGRESS`

**Goal:** 完成 save/load、config、backlog、gallery、replay、route chart、voice replay 和 localization preview 的系统 UI 数据模型和 gate。

**Depends On:** `S3-CORE-03`、`S3-LUAU-02`、`S2-UI-BACKEND-01`、`S3-UI-SCRIPT-01`、[AstraVN System UI Profile](../../modules/astra-vn-system-ui-profile.md)

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-system/src/system_ui.rs`、`Engine/Source/Modules/AstraVN/astra-vn-package/tests/commercial_baseline.rs`、`Engine/Source/Modules/AstraVN/astra-vn-system/tests/system_controls.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs`、`Examples/NativeVN/system.astra`

**Steps:**

1. 定义 `SystemStoryManifest`、save slot metadata、config schema、unlock source、route chart graph 和 localization preview schema。
2. 让 Luau policy 只负责页面流程和视觉策略，Core 继续持有 save/backlog/read-state/voice replay 权威状态。
3. 实现 system page reachability、return-to-savepoint、migration、gallery/replay unlock source 和 font fallback 检查。
4. 编写 save/load、config invalid key、backlog voice replay、gallery unlock、route chart 和 localization preview 测试。
5. Classic/Modern 的全部页面只走 `.astra` Blueprint、Rust ViewModel、Luau Controller、Yakui 和 Scene2D；旧固定矩形 hit-test 已删除。

**Done Evidence:** `cargo test -p astra-vn-package --test commercial_baseline`、`cargo test -p astra-vn-system --test system_controls`、`cargo test -p astra-release --test release_report release_gate_` 和 `cargo test -p astra-test --test vn_scenario` 通过；`vn.system_ui_profile` 会阻断缺入口、缺 policy、缺 `vn.system_ui_profile_manifest`、schema 无 migrator、gallery/replay unlock source 缺失和 localization coverage 缺口，并在通过时输出 page count、unlock source count、localization locale count 和 save migrator evidence。

**Linked Test IDs:** `T-S3-SYSTEM-01`

## S3-UI-SCRIPT-01 AstraVN UI Blueprint 与 Controller

**ID:** `S3-UI-SCRIPT-01`

**Status:** `IN_PROGRESS`

**Goal:** 用 `.astra` 声明 backend-neutral View/Binding/Action，以 Rust ViewModel 和 typed Luau Controller 驱动 Yakui。

**Depends On:** `S3-SCRIPT-02`、`S3-LUAU-02`、`S2-UI-BACKEND-01`、[ADR 0016](../../adr/0016-astravn-script-declared-ui.md)

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-script/`、`Engine/Source/Modules/AstraVN/astra-vn-ui/`、`Engine/Source/Modules/AstraVN/astra-vn-ui-yakui/`

**Steps:**

1. 增加 `Story`/`Ui` source role、UI Typed AST、semantic passes 和 `CompiledVnProject`。
2. 实现 `ui_view`、`ui_bind`、stable semantic id、typed action 和 binding resolver。
3. 接入生成 `.d.luau`、`luau-analyze` 和可序列化 Controller effect。
4. 迁移全部 caller/package/target，删除旧 compile API 与 reader。

**Done Evidence:** compiler、Cook、Player、sample、Editor adapter 和 bundle 只使用 `CompiledVnProject`；旧 API/section/target 被明确拒绝，Windows/Web 产品页 evidence 通过。

**Linked Test IDs:** `T-S3-UI-SCRIPT-01`

## S3-UI-EXT-01 UI component plugin ABI

**ID:** `S3-UI-EXT-01`

**Status:** `IN_PROGRESS`

**Goal:** 允许作品专属组件挂载到静态 typed slot，同时阻断 Yakui/native handle、未签名 artifact、越界 DTO 和 authority leakage。

**Depends On:** `S1-DYLIB-01`、`S2-UI-BACKEND-01`、`S3-UI-SCRIPT-01`、[UI Component Plugin Contract](../../contracts/ui-component-plugin.md)

**Target Paths:** `Engine/Source/Runtime/astra-ui-plugin-abi/`、`Engine/Plugins/Fixtures/`、Windows/Web Cook 与 bundle path

**Steps:**

1. 定义 provider/session/component lifecycle 和 bounded DTO。
2. 实现 Windows Ed25519 signer allowlist 与 Web WIT/jco artifact pipeline。
3. 实现 host capability、用户手势、hard limit 和 failure termination。
4. 用签名 Windows/Web fixture 验证 ABI；产品页不得依赖 fixture。

**Done Evidence:** lifecycle、signature、fingerprint、permission、bounds、restore、panic/trap/timeout 和 redaction gate 全部通过；失败不 fallback。

**Linked Test IDs:** `T-S3-UI-EXT-01`

## S3-ADVANCED-01 Advanced presentation opt-in profile

**ID:** `S3-ADVANCED-01`

**Status:** `IN_PROGRESS`

**Goal:** 建立旗舰演出 profile，覆盖多层 stage、camera、video layer、shader/filter、voice sync、复杂 text effect、skip/auto/replay 和 fallback。

**Depends On:** `S3-PRESENT-01`、`S3-SYSTEM-01`、`S2-MEDIA-04`

**Target Paths:** `Examples/NativeVN/`、`Engine/Source/Modules/AstraVN/astra-vn-package/tests/advanced_presentation.rs`、`Engine/Source/Programs/astra-cli/tests/target_platform.rs`；旗舰项目的正式 scenario 待 Runtime/Player 基座门禁关闭后建立。

**Steps:**

1. 建立 opt-in sample project，绑定 advanced Luau policy、standard command provider 和 system story manifest。
2. 覆盖多层 stage、camera keyframe、video layer、shader/filter fallback、voice sync 和 text effect。
3. 编写 full scenario，穿过 system UI、save/load、replay 和 release gate。
4. 接入 `vn.advanced_presentation`、`presentation.fallback`、`renderer.effect_budget` 和 `timeline.join_cancel` evidence。

**Current Evidence:** `cargo test -p astra-vn-package --test advanced_presentation` 覆盖 advanced profile 的 package contract；Windows/Web formal runner 尚未形成真实宿主证据，因此本项仍为 `IN_PROGRESS`。

**Linked Test IDs:** `T-S3-ADVANCED-01`

## S3-EDIT-01 Graph/Timeline 同源 metadata

**ID:** `S3-EDIT-01`

**Goal:** Graph 和 Timeline 只保存作者 metadata，必须能回写或编译到同一 IR、source map 和 debug symbol。

**Depends On:** `S3-SCRIPT-02`

**Target Paths:** `Engine/Source/Modules/AstraVN/astra-vn-editor/src/editor_metadata.rs`、`Engine/Source/Modules/AstraVN/astra-vn-editor/tests/editor_metadata.rs`

**Steps:**

1. 定义 Graph node metadata、Timeline track metadata、wait/fence metadata 和 command id binding。
2. 实现 metadata -> `.astra` patch 或 policy override 的稳定回写路径。
3. 校验 metadata 不产生第二套 runtime model。
4. 编写 graph roundtrip、timeline fence、wait state source map 和 source map identity 测试。

**Done Evidence:** `cargo test -p astra-vn-editor --test editor_metadata` 通过；Graph/Timeline 修改后仍指向同一 command id，缺 command 会阻断，patch manifest 只输出 source map 已知 command id，wait/fence source map 保持一致。

**Linked Test IDs:** `T-S3-EDIT-01`

## S3-SAMPLE-01 Commercial baseline sample 与 full playthrough

**ID:** `S3-SAMPLE-01`

**Status:** `IN_PROGRESS`

**Goal:** 建立 NativeVN commercial baseline sample 和 full playthrough scenario。

**Depends On:** `S3-CORE-03`、`S3-LUAU-02`、`S2-GATE-01`

**Target Paths:** `Examples/NativeVN/`、`scenarios/full_playthrough.yaml`、`Engine/Source/Programs/astra-cli/tests/target_platform.rs`

**Steps:**

1. `Examples/NativeVN/project.yaml` 声明 `nativevn-game`、classic/modern profile、`.astra` source 和 `scenarios/full_playthrough.yaml` refs。
2. `astra cook` 编译 `.astra` 并输出 `CompiledStory`、profile、policy、extension、standard command、presentation provider、commercial baseline 和 system story package artifacts。
3. `scenarios/full_playthrough.yaml` 覆盖启动、movie/voice wait、choice、路线、系统页、save/load、config、gallery/replay unlock 和 replay_from_start。
4. `astra package validate` 验证 `vn.commercial_baseline` pass，scenario 断言 backlog、read-state、voice replay、system state、route coverage 和 replay hash。

**Done Evidence:** `cargo test -p astra-cli --test target_platform nativevn_minimal_profile_cooks_packages_and_runs_headless` 通过；该测试执行真实 cook、package、release validate 和 headless full playthrough。

**Linked Test IDs:** `T-S3-SAMPLE-01`

## S3-GAME-TARGET-01 NativeVN Game target

**ID:** `S3-GAME-TARGET-01`

**Status:** `IN_PROGRESS`

**Goal:** NativeVN sample 以 `Game` target 完成 cook、package、full playthrough 和 release gate。

**Depends On:** `S2-TARGET-GATE-01`、`S3-SAMPLE-01`

**Target Paths:** `Examples/NativeVN/project.yaml`、`scenarios/full_playthrough.yaml`、`Engine/Source/Programs/astra-cli/src/main.rs`、`Engine/Source/Programs/astra-cli/tests/target_platform.rs`

**Steps:**

1. 在 sample project 中声明 `nativevn-game`，绑定 `classic` 和 Stage 3 平台列表 `headless`、`windows`、`web`。
2. `astra cook`、`astra package build`、`astra test run` 和 `astra package validate` 全部使用 `--target nativevn-game`。
3. Release report 同时包含 `target.manifest`、`vn.commercial_baseline`、`vn.system_ui_profile` 和 `platform.eligibility`。
4. 编写 Game target package、standalone Windows/Web bundle 和 full playthrough 测试。

**Current Evidence:** Engine workspace 不再运行旗舰 Windows/Web bundle route 测试。`minimal` 只验证 cook/package 与 Headless 产品链路；正式 Game target、bundle 和平台 E3 仍为 `IN_PROGRESS`。

**Linked Test IDs:** `T-S3-GAME-TARGET-01`

## S3-PLAYER-AUTOMATION-01 Windows/Web live player automation

**ID:** `S3-PLAYER-AUTOMATION-01`

**Status:** `IN_PROGRESS`

**Goal:** Windows 和 Web player gate 必须通过平台原生输入推进 dialogue、choice、system page、config、save/load、backlog 和 route，不允许用 direct runtime command 或 route runner 冒充玩家输入。

**Depends On:** `S3-GAME-TARGET-01`、`S3-SAMPLE-01`、`S3-SYSTEM-01`、`S3-PRESENT-01`

**Target Paths:** `Engine/Source/Runtime/astra-player-core/`、`Engine/Source/Programs/astra-player/`、`Engine/Source/Developer/astra-release/tests/release_report.rs`、[AstraVN Live Player Automation](../../implementation/astra-vn-live-player-automation.md)

**Steps:**

1. 定义 `astra.player_automation_script.v1`、`astra.player_input_transcript.v1` 和 `astra.player_automation_report.v1`，只记录 hash、region id、event source、focus state、meter summary、host evidence 和 diagnostic。
2. 新增 shared player core，校验 automation script/transcript/report，禁止 live-player gate 直接调用 `VnPlayerCommand`、DOM click、JS callback 或 `--route-scenario`。
3. Windows driver 发现并 focus player window，用 Win32 `SendInput` 注入 mouse/keyboard，确认 winit event loop 收到事件，并采样真实窗口或 renderer readback 与 AudioGraph/WASAPI meter。
4. Web driver 启动本地 HTTP server 和真实 Chrome/Edge 页面，用 CDP `Input.dispatchMouseEvent`、`Input.dispatchKeyEvent` 和必要时 touch event 注入输入，并采样 canvas/screenshot 与 WebAudio meter。
5. Shared verifier 要求 dialogue、choice、system page、config、save/load、backlog 和 route check 都由平台输入触发；发现 `--route-scenario` 自推进、`--dump-dom`、DOM `element.click()`、JS callback 或 API 可用性 smoke 时必须 blocking。

**Current Evidence:** `cargo test -p astra-player-core`、`cargo test -p astra-player --test windows_input_automation`、`cargo test -p astra-player --test web_input_automation` 和 `cargo test -p astra-release release_gate_accepts_player_full_playable_only_with_matching_live_report` 通过；release gate 只有在显式传入匹配 package hash/profile/target 的 `astra.player_automation_report.v1` 时才让 `player.full_playable` pass，Windows transcript 只接受 `sendinput.*`，Web transcript 只接受 `cdp.*`，并要求 live input 有 player host consumed trace；visual comparison evidence 缺失会 blocking，direct `route_scenario`、DOM click、JS callback 和 direct `VnPlayerCommand` 会 blocking。Windows runner 已能启动 bundle live window、执行 `SendInput`、捕获 player host `TRACE` consumed log、采集 client region hash、绑定视觉 comparison report hash，并要求同 package/session 的 host conformance `audio.output_meter` callback evidence；当前私有 TsuiNoSora acceptance 的 report 已证明 28 次 `SendInput` 被窗口消费，但仍因输入后画面未变化和 full route coverage 缺失而 blocking。Web browser host run、真实 VN state 推进和同次 full route evidence 仍是下一步 acceptance。

**Linked Test IDs:** `T-S3-PLAYER-AUTOMATION-01`

## S3-FLAGSHIP-DEMO-01 NativeVN flagship demo

**ID:** `S3-FLAGSHIP-DEMO-01`

**Status:** `IN_PROGRESS`

**Goal:** 交付 15–20 分钟、三终局、中英双语和正式原创资产的旗舰 Demo，并直接替换 `Examples/NativeVN`。当前以用户授权中文配音的发行形态接入真实 `.astra`、UI、localization、asset sidecar 和 Cook。

**Depends On:** `S3-SAMPLE-01`、`S3-PLAYER-AUTOMATION-01`、[NativeVN Flagship Demo Migration](../../migrations/nativevn-flagship-demo-migration.md)

**Current Evidence:** 旗舰项目包含 180 条中英文对白、共通线和三个唯一终局、79 张视觉文件、9 张 UI 视觉稿、12 秒可重建视频、4 首 BGM、3 个 stinger、18 个 SE、180 条用户授权中文配音、SVG icon、alt text、prompt/provenance、manifest 和 review。真实项目进一步提供 `.astra` canonical story、双语 runtime localization、Yakui UI source、theme/controller、283 个 asset sidecar、显式 target/provider binding 和 package section。`python Tools/NativeVN/validate_content_pack.py` 与工具单元测试覆盖结构、双语引用、路线、媒体、hash、透明通道、音频、逐 cue 配音绑定、授权状态、项目绑定和公开树安全；OpenRouter 辅助试听报告仍不替代人工听审。状态为 `content_creation=complete`、`public_release_assets=ready_with_authorized_voice`、`engine_integration=cook_ready_with_voice`。本轮只形成真实 Cook/package evidence，不执行 Runtime 或 Player 测试；formal Windows/Web E3 前不得标为 `DONE`。

## 跨 Stage Observability follow-up

AstraVN compiler/core/policy/presentation/provider、Player input/route/automation 已纳入 `OBS-CORE-01` 的 category/span/session 日志。该日志只用于定位 `S3-PLAYER-AUTOMATION-01`，不能替代同 run 视觉、音频、host 和 route evidence，也不会改变 Stage 3 的 `IN_PROGRESS` 状态。

# Data, Save, Package Contract

Astra 源数据 text-first，运行时数据 binary-first。YAML 适合人写，二进制容器适合发布和加载。

## Source Descriptors

项目、插件、资产 sidecar 和测试 scenario 使用 YAML：

```yaml
schema: astra.project.v1
id: com.example.nativevn
engine_modules:
  renderer2d: astra.renderer.wgpu
  text_layout: astra.text.cosmic
  audio: astra.audio.platform
platforms: [windows, linux, macos, ios, android, web]
targets:
  - id: nativevn-game
    kind: game
    crate: astra-vn
    runtime_provider: native_vn
    default_profile: desktop-release
    platforms: [windows, linux, macos, ios, android, web]
    packaged: true
```

每个 YAML schema 必须有 Rust 类型、schema version、migrator 和验证命令。

`astra.asset.v1` 的 Rust 真源是 `AssetSidecar`。除 source/type/license/importer/cook/review 外，它包含默认空的 typed `dependencies: Vec<AssetId>`；font asset 还必须提供 `FontAssetMetadata` 的 family、face index、可选 subset 与有序不重叠 Unicode scalar coverage，缺 metadata、空 family、非法 range 或把 font metadata挂到非字体资产都会 blocking。self、duplicate、missing dependency 和 cycle 分别在 sidecar 或 batch graph 边界阻断。Cook 输出 `astra.cook_manifest.v2`，其中 `asset_cook` 使用 `astra.cook_batch_summary.v1`，只记录 content-bound graph hash、artifact/cache-hit/cooked count 和显式 concurrency limit。NativeVN Cook 从 typed font metadata自动生成 profile-bound `astra.font_manifest.v1`，从适用的 localization section 和 `nativevn.default_locale` 生成 `astra.player_locale_config.v1`；不允许项目手写同名 font manifest覆盖权威结果。Package 把同一摘要写入 required `cook.summary` section；`PackageReader` 要求摘要计数与 `astra.cooked_asset.v1` section 数量一致。旧 cook manifest、缺 summary、计数不闭合或 graph identity无法解析时必须重新 cook，不执行隐式迁移或 fallback。

Stage 3 cook 已支持 project-level `package_sections` descriptor，用于把脱敏 JSON report 或 manifest 作为 `Raw` section 放入 package。descriptor 只允许相对 `path`，可用 `targets` 和 `profiles` 过滤，不允许记录本地绝对路径或商业 payload。TsuiNoSora gate 用它把 `tsuinosora.reference_evidence`、`tsuinosora.asset_analysis`、`tsuinosora.conversion_manifest`、`tsuinosora.mount_policy`、`tsuinosora.modern_profile_report` 和 formal release `tsuinosora.manual_signoff` 接入 package/release gate；`tsuinosora.extract_report.v1`、`tsuinosora.director_resource_map.v1`、`tsuinosora.director_cast_map.v1`、`tsuinosora.director_lingo_map.v1`、`tsuinosora.cast_source_map_report.v1`、`tsuinosora.script_source_map.v1`、`tsuinosora.script_source_map_report.v1`、`tsuinosora.route_graph_report.v1` 和 `tsuinosora.native_asset_rearrange_report.v1` 是本地 gate preflight input/report，只记录 sidecar 抽取预检、Director `imap`/`mmap` resource map、受限 `XFIR` exact wrapper 中的 RIFF/RIFX payload hash/size、Director `KEY*`/`CAS*` cast map、Director `Lctx`/`Lnam`/`Lscr` Lingo map、`Lnam` entry count/table hash、受限 RIFF/RIFX readable chunk 证据、opaque/compressed、declared size mismatch 或尾随未验证 bytes Shockwave blocking diagnostic、cast member/source hash、Director child resource id/FourCC/hash、route id/source line/hash/diagnostic、native-assets source/native path/hash/byte size 和未解析 container 阻断；declared size mismatch 时不记录 resource/tag coverage，不作为发布 package section。

补充：`tsuinosora.director_lingo_map.v1` 还记录 `Lctx` entry count/table hash；`Lctx` 和 `Lnam` 都不得输出原始 table payload 或 name/context 字符串。`Lctx` 未按 32-bit entry 对齐、`Lnam` 未按 null-terminated table 证明边界时必须 blocking。

补充：`tsuinosora.director_cast_map.v1` 遇到同一 `CASt` resource 被多个 `CAS*` library/slot 绑定时必须 blocking，避免不唯一 cast member 继续进入 `tsuinosora.cast_source_map_report.v1`。

补充：`tsuinosora.cast_source_map_report.v1` 必须阻断 `tsuinosora.cast_map.v1` 或外部 `tsuinosora.director_cast_map.v1` sidecar 中的正文、payload 或 bytecode 字段；报告只允许记录字段路径、hash、id 和 diagnostic。

补充：`tsuinosora.director_cast_member_metadata.v1` 只能作为 `CASt` payload 内显式脱敏 metadata 使用，允许字段限于 kind、route id、command id、anchor、bounds 和 metadata hash；读取结果可以进入 `tsuinosora.director_cast_map.v1` 和 `tsuinosora.cast_source_map_report.v1`，但原始 `CASt` payload 不得进入 report。

补充：`tsuinosora.director_cast_member_metadata.v1` 的 anchor 必须是数值 `x`/`y`，bounds 必须是非负数值 `x`/`y`/`width`/`height`；layout 字段不能验证时必须 blocking。

补充：`tsuinosora.director_cast_member_metadata.v1` 的 `kind: character_atlas` 必须携带 parts；part id、pose、expression、layer 和 fallback 必须是 safe symbol，anchor/crop 必须是数值，mouth/eye state compatibility 必须是 boolean。合规 parts 可以进入 `tsuinosora.director_cast_map.v1` 和 `tsuinosora.cast_source_map_report.v1`。

补充：`tsuinosora.route_graph_report.v1` 和 `tsuinosora.script_source_map_report.v1` 必须阻断同一 `route_id` 的 terminal/choice signature 冲突；否则 NativeVN package input 不允许生成 `.astra` story 或 scenario refs。

补充：同一 route 内的 `choices` 必须唯一；重复 choice id 会阻断 route graph/script source-map report，避免生成重复 `player_input choose` 或 `.astra` option key。

补充：script source-map fallback 只适用于 route graph 缺失；如果 `tsuinosora.route_graph.v1` sidecar 存在但校验失败，`tsuinosora.route_graph_report.v1` 的 diagnostic 必须继续阻断 Stage 3 gate。

补充：`tsuinosora.script_source_map.v1` 在覆盖 unsupported `Lscr` bytecode 时，route 只能用 `script_resource_id` 和 `script_payload_sha256` 绑定 `tsuinosora.director_lingo_map.v1` 中的 Lscr resource；缺失、未知 resource 或 hash mismatch 必须 blocking，schema 仍禁止脚本文本、bytecode、payload 和本地路径。

补充：`tsuinosora.nativevn_package_input_report.v1` 会对显式 route 输入执行同样的安全校验；不安全 symbol、非 covered route、重复 choice 或冲突 route signature 必须在写 `.astra` story 和 scenario refs 前阻断。

补充：`tsuinosora.local_gate_report.v1` 不把显式 routes 作为商业 route coverage evidence；`local-gate` 必须从 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 派生 routes 后才允许写 `tsuinosora.nativevn_package_input_report.v1`。

AI ModelBundle source descriptor 也是 YAML，但只作为 cook 输入。Cook 后，Shipping Runtime 只能通过 Asset VFS section 读取模型资源：

```yaml
schema: astra.ai_model_bundle.v1
id: com.example.model.local_director
provider: astra-ai-onnx
distribution: bundled
pipeline: llm
platforms: [windows, linux, macos, ios, android, web]
```

descriptor 可以引用项目内相对模型资源、tokenizer、runtime recipe 和 custom op sidecar；不得引用本地绝对路径。Cook 产物必须写入 `ai.model_bundle_manifest` 和对应 Asset VFS content entry，不通过 project-level `package_sections` 携带模型 payload。

## Binary Container

Save 和 package 共用自描述容器结构：

```text
AstraContainerHeader
SectionTable[]
SectionPayload[]
FooterHash
```

Section payload 默认使用 `postcard` + serde。大型媒体 payload 可以使用 `Raw` 或 `Zstd` section codec；section table 必须记录 codec、hash、stored hash、offset、length、decoded length 和 migration policy。

容器 ABI 在 [Package And Save](../implementation/package-save.md) 中锁定：little-endian、8 byte alignment、header magic、section table、schema id、codec、hash、optional encryption descriptor、migration policy 和 footer hash。Encryption descriptor 只描述 provider 能力，不提供 DRM 或访问控制绕过方案。

## Save

Save 必须包含 Runtime state、Actor/Component、StateMachine、Blackboard、Director、AwaitToken、script snapshot、VN backlog、AudioGraph state、FilterGraph state、committed AI output、plugin opaque sections 和 migration manifest。NativeVN product provider 只输出 `runtime.world`/`astra.runtime.save_blob.v2` 权威 section；其 Raw payload 是自描述 Runtime save container，VN runtime/policy component 连同完整 Event/Await/delayed queue、MutationLog 和 effect trace 一起进入 `runtime.world` snapshot。旧的拆分 `vn.runtime_state`/`vn.policy_state` 不能作为 product save authority。

AI Runtime 生成的文本、图像和语音结果是 save 数据，不是 package 数据。流式 chunk 通过 `ai.generated_artifact.*` extra section 固化；manifest 记录 model fingerprint、provider profile、validator result、content type、hash、codec 和可选 encryption。正式 replay 只读 save payload，不重跑 provider。

## Package

Package 必须包含 cooked assets、compiled `.astra` IR、Luau policy bundle、policy lock/source cache、schema registry、provider policy、module fingerprint、target manifest、release report summary、test scenario references 和 platform eligibility。Runtime 不依赖源 YAML 启动。

Package 通过 Asset VFS 暴露为 package-backed mount。`.astrapkg` 仍是控制面和证据面容器；legacy pack、local authorized source 和 overlay source 必须通过 VFS mount descriptor 表达，不能替代 package/save container。

## Standalone Bundle

Standalone bundle 由已 cook/package 的 `.astrapkg` 生成，不从源 YAML 直接拼装。`astra.standalone_bundle_manifest.v2` 只记录 target、profile、platform、entrypoint、package hash、scenario refs、artifact role/hash/byte size 和相对文件清单。Windows bundle 必须显式传入已构建的 Player 与 Crash Reporter；Web bundle 必须显式传入 WASM、loader 与 AudioWorklet。Bundle 不生成 Web route model 或 JavaScript scenario/runtime。报告不得记录本地绝对路径、用户名、商业 payload、正文、截图、音频或影片。

`astra.player_route_report.v1` 是已实现的 route/report slice，不能满足 `player.full_playable`。Stage 3 完整可玩 gate 还需要 `astra.player_automation_script.v1`、`astra.player_input_transcript.v1` 和 `astra.player_automation_report.v1`，证明 Windows/Web player 由平台原生输入推进，并在同一次 run 中产出视觉变化、音频 meter、host evidence 和 route evidence。

当前已落地的 bundle slice：

| Schema | Status | Purpose |
| --- | --- | --- |
| `astra.standalone_bundle_manifest.v1` | `DONE` | Windows bundle 写入 `AstraPlayer.exe`、`AstraPlayer.config.json`、package 和 scenario refs；Web bundle 写入 `index.html`、`astra-player.js`、`AstraPlayer.config.json`、`AstraPlayer.route_model.json`、package、scenario refs 和 scenario JSON refs；manifest 只使用相对路径和 hash |
| `astra.player_launch_report.v1` | `IN_PROGRESS` | Windows entrypoint 无参数启动时读取 bundle manifest、校验 package hash 和 target manifest，输出 machine-readable readiness report |
| `astra.player_route_report.v1` | `DEPRECATED_DIAGNOSTIC` | 旧 Windows/Web route runner 不再作为 bundle runtime 或 Migration 8 evidence；`player.full_playable` 只接受真实 host input/visual/audio/route automation |
| `astra.player_automation_script.v1` | `SPEC_READY` | 描述 launch、click、key、wait、screenshot sample、audio sample 和期望 route/system UI state；只允许公开 scenario 相对路径、target region id、key 名、等待条件和 check id |
| `astra.player_input_transcript.v1` | `SPEC_READY` | 记录平台事件来源、坐标、按键、target region、frame hash before/after、focus state、event-loop receipt 和 diagnostic；不得记录本地路径、截图 payload、音频 payload 或 native handle |
| `astra.player_automation_report.v1` | `SPEC_READY` | 聚合 input transcript、visual report、audio report 和 route report；只记录 hash、region id、check id/status、event source、focus state、meter summary、host evidence 和 diagnostic |
| `astra.player_presentation_report.v1` | `WINDOWS_PATH_DONE` | 从 `PlayerHostCommand::PresentScene` 的同 run hardware capture 生成，绑定 target/profile/package/profile hash/build/session、renderer/font provider、layout/command/capture hash、frame sequence、尺寸与变化像素；headless、空画面或 identity drift blocking，Web 与 bundled VN 完整产品主路径仍开放 |
| `astra.player_locale_config.v1` | `WINDOWS_PATH_DONE` | Cook 从 profile-eligible `vn.localization.<locale>` sections生成 default/available locale identity；bundle config携带 default locale，Player按该 locale读取同包 localization。缺 default、default不在 available、locale id不安全、section/schema/locale漂移或重复 key均 blocking |
| `astra.performance_budget.v1` | `DONE` | profile owner 声明 target/profile/hash、最短 run、metric unit、sample capacity 与 percentile/min/max threshold；未知 metric、重复 metric、零容量或非单调阈值 blocking |
| `astra.performance_report.v1` | `IN_PROGRESS` | bounded recorder 生成 source/package/build/session-bound min/p50/p95/p99/max、sample count 和 diagnostic；Windows native media 与 release same-run validator 已接入，真实 Player artifact 和正式 reference pass 尚未闭合 |
| `astra.text_layout_replay.v1` / `astra.text_layout_replay_snapshot.v1` | `IN_PROGRESS` | bounded postcard transcript 固化 package/build/session/provider/font/request/layout/glyph identity，支持事务性 restore continuation 与 provider-free replay；Windows Player command/release consumer已闭合，bundled VN 的 dialogue/choice/system text 已接入，Web 与 bundled VN 完整 presentation/audio 主路径尚未闭合 |
| `astra.open_font_fixture_manifest.v1` | `DONE` | hermetic 字体回归清单，固定 upstream revision、OFL、source URL、family/face、coverage、byte size 和 SHA-256；只用于测试 provenance，不替代产品 `astra.font_manifest.v1` |
| `astra.windows_gpu_glyph_golden.v1` | `WINDOWS_E3_REFERENCE` | 固定 hardware wgpu glyph atlas 的 font revision、layout hash、capture hash、画布、背景和最小变化像素；只证明 Windows text pass，不替代 Web、Player 或完整 SceneCommand renderer evidence |

Migration 11 planned Headless 测试格式不进入 package 或发布 profile：

| Schema | Status | Purpose |
| --- | --- | --- |
| `astra.headless_host_profile.v1` | `SPEC_READY` | 测试专用 provider binding、input/artifact policy、资源限额与 build/package identity；shipping API 必须拒绝 |
| `astra.user_input_sequence.v1` | `SPEC_READY` | 平台无关物理输入、固定 tick/time、await、checkpoint 和 shutdown；禁止产品语义直调 |
| `astra.headless_protocol.v1` | `SPEC_READY` | 文件和 stdio 共用的双向 JSONL envelope，包含 session 与严格递增 sequence |
| `astra.headless_artifact_manifest.v1` | `SPEC_READY` | PNG/WAV 相对路径、hash、尺寸、色彩空间、采样率、声道、时长、sequence、checkpoint 与 provider identity |
| `astra.headless_run_report.v1` | `SPEC_READY` | Headless host、输入、产物、自动比较和 blocking diagnostic |
| `astra.headless_review.v1` | `SPEC_READY` | required checkpoint 与音频工具审查；记录 artifact hash、reviewer/tool identity 和 verdict，不记录媒体内容 |
| `astra.headless_preflight_link.v1` | `SPEC_READY` | 关联同 build/package/input/scenario/target/content identity 的 Headless 与真实平台 session |

这些 schema 当前只有文档，不得当作已生成报告。商业或本地 PNG/WAV 只能留在 ignored 私有工作区；可提交报告只保留脱敏 metadata。

Stage 3 Windows `player.full_playable` required evidence 是 `player.window.focused`、`player.input.sendinput.mouse`、`player.input.sendinput.keyboard`、`player.visual.window_regions`、`player.audio.wasapi_meter` 和 `player.route.full`。Web required evidence 是 `player.browser.cdp_session`、`player.input.cdp_mouse`、`player.input.cdp_keyboard`、`player.visual.canvas_regions`、`player.audio.webaudio_meter` 和 `player.route.full`。缺 input transcript、缺截图区域像素变化、缺音频 meter、缺平台 host evidence，或发现 `VnPlayerCommand`、`--route-scenario` 自推进、`--dump-dom` route runner、DOM `element.click()`、直接 JS callback 或 direct runtime command path 时，`player.full_playable` 必须 blocking。`input.browser`、`input.gamepad` 这类 API 可用性只能作为 capability，不能作为 playable evidence。

Stage 3 已开始落地的 VN package sections：

| Section | Status | Purpose |
| --- | --- | --- |
| `vn.compiled_story` | `IN_PROGRESS` | `postcard` 编码的 `CompiledStory`，当前包含 StoryManifest、VariableManifest、CommandManifest、source map、debug symbol 和 route graph；release gate 会在 classic/modern profile 下校验 schema、story/state 和 route graph evidence |
| `vn.profile_manifest` | `IN_PROGRESS` | `postcard` 编码的 profile/target manifest，当前 release gate 会校验 validation profile 和 selected target |
| `vn.policy_bundle_manifest` | `DONE` | `postcard` 编码的 `VnPolicyBundleManifest`，release gate 会校验 standard policy bundle、required capabilities、sha256 lock hash、source hash、byte size 和 source cache section |
| `vn.policy_bundle_source_cache` | `DONE` | `postcard` 编码的 `VnPolicyBundleSourceCache`，包内固定官方 Luau source，release report 只输出 section id、hash/size evidence 和 diagnostic，不输出 Luau source payload |
| `vn.extension_manifest` | `IN_PROGRESS` | `postcard` 编码的 `VnExtensionManifest`，当前 release gate 会校验 policy bundle、VN command、presentation command、editor metadata 和 release check provider 显式绑定 |
| `vn.standard_command_manifest` | `IN_PROGRESS` | `postcard` 编码的 `VnStandardCommandManifest`，当前 release gate 会校验 standard command descriptor、compiled presentation command usage、必需属性和 movie fallback |
| `vn.presentation_provider_manifest` | `IN_PROGRESS` | `postcard` 编码的 `VnPresentationProviderManifest`，当前 release gate 会校验 renderer/filter provider id、shader profile、fallback policy 和 movie/voice/timeline await capability |
| `vn.commercial_baseline_manifest` | `IN_PROGRESS` | `postcard` 编码的 `VnCommercialBaselineManifest`，当前 release gate 会校验商业 VN 自动化基线 feature coverage，不包含商业 payload |
| `vn.advanced_presentation_manifest` | `DONE` | `postcard` 编码的 `VnAdvancedPresentationManifest`，仅 advanced opt-in profile 阻断；记录 story hash、timeline id 和 `stage.multi_layer`、`camera.task`、`video.layer`、`timeline.join_cancel`、`presentation.fallback`、`voice.sync`、`renderer.effect_budget` evidence |
| `vn.system_story_manifest` | `DONE` | `postcard` 编码的 `SystemStoryManifest`，release gate 会校验商业 VN 必需 system page entry 和 policy binding |
| `vn.system_ui_profile_manifest` | `DONE` | `postcard` 编码的 `VnSystemUiProfileManifest`，记录 save migration、gallery/replay unlock source 和 localization coverage release evidence |
| `scenario.refs` | `DONE` | package 内 scenario 引用表；Stage 3 VN scenario 已复用该 section |
| `tsuinosora.reference_evidence` | `IN_PROGRESS` | TsuiNoSora 视觉参考证据 hash、尺寸、区域 id 和布局指标；默认 `Title.png`/`Game.png` 固定尺寸/hash mismatch 会 blocking；release gate 会阻断缺 section、schema mismatch、非 `pass` 状态、路径泄露和 payload-like 字段泄露 |
| `tsuinosora.asset_analysis` | `IN_PROGRESS` | synthetic fixture 已覆盖脚本引用、container source、use timing、visible bbox、edge padding、颜色分布、重复 hash、reference match、classification count、atlas crop/part 和分类冲突 quarantine；release gate 会阻断空 asset evidence、quarantine、schema/status 错误、路径泄露和 payload-like 字段泄露；真实本地 gate 仍未完成 |
| `tsuinosora.conversion_manifest` | `IN_PROGRESS` | 本地转换 coverage、source map、converted resource evidence、missing/quarantine/manual review summary；release gate 会阻断 route coverage 缺口、空 converted resource evidence、resource 缺 source/native/classification/hash/byte size、schema/status 错误、路径泄露和 payload-like 字段泄露 |
| `tsuinosora.mount_policy` | `IN_PROGRESS` | patch target 的本地挂载 alias、hash policy 和 fallback 规则；release gate 会阻断 target 不匹配、alias 为空、schema/status 错误、路径泄露和 payload-like 字段泄露；standalone bundle 会写入脱敏 `AstraPlayer.mount_policy.json`，Windows/Web route report 必须校验 `player.mount_policy` 和 `player.mount_policy_hash`，patch target 还必须校验 `player.patch_direct_read`；Windows patch direct-read 必须有 scenario `mount_probes` 或 route-bound `mount_assets` 加 `--mount-root alias=path` 的本地读取证据，`mount_assets.role` 必须是 Asset analysis 允许分类且不能是 `unknown` 或 `script`，report 只记录 `player.patch_mount_probe`、`player.patch_mount_asset` check 和状态，不记录本地 root；Web player 遇到本地 mount probe/asset scenario 必须 blocking |
| `tsuinosora.modern_profile_report` | `IN_PROGRESS` | `modern` profile 的增强开关、fallback hash 和 core-state 隔离 evidence；release gate 会阻断缺 section、不可回退增强、schema/status 错误、路径泄露和 payload-like 字段泄露 |
| `tsuinosora.manual_signoff` | `IN_PROGRESS` | formal release profile 的人工验收摘要，只记录 `check_id`、result、blocker count 和脱敏规则；release gate 会阻断缺 section、缺 required check、错误 check id 字段、失败项、blocker、schema/status 错误、路径泄露和 payload-like 字段泄露 |

Stage 3 已开始落地、但尚未全部写入 release package 的 VN runtime 数据：

| Data type | Status | Purpose |
| --- | --- | --- |
| `VnRuntimeState` | `IN_PROGRESS` | 保存 profile、locale、current story/state、command cursor、call stack、pending choice、变量、backlog、read-state、voice replay、route coverage、route flags 和 `VnSystemState` |
| `astra.runtime.save_blob.v2` | `DONE` | NativeVN product provider 的唯一 `runtime.world` save section；nested container 保存完整 RuntimeSnapshot，并通过 restored step/seed 约束 continuation |
| `VnRuntimeStateSave` | `REFERENCE_ONLY` | `astra-vn-save` 的局部 VN state 工具；不得替代 product provider 的完整 RuntimeWorld save authority |
| `BacklogEntry` / `VnReplayUiState` | `DONE` | 保存 command id、text key、speaker、voice ref、story/state、route position、read flag、layout metadata、voice replay rows 和 replay UI hash |
| `VnSystemState` | `IN_PROGRESS` | 保存 auto enabled、skip mode、config key/value、gallery unlocks 和 replay unlocks；随 save/load/replay 保持 hash 一致 |
| `VnPolicyState` | `DONE` | 保存 Luau policy 可见变量、`astra.mutate` mutation trace、previous value、rollback/playback metadata、`astra.command` capability request trace、`astra.query` read-only trace、`astra.trace` diagnostics 和 serializable snapshots；不保存 function、thread、userdata 或 native handle |
| `VnPolicyStateSave` | `DONE` | Runtime save section `vn.policy_state`，保存 policy state hash、mutation trace、rollback/replay event metadata、command/query/trace records 和 serializable snapshots |
| `VnPolicyBundleManifest` | `DONE` | 保存 policy bundle id、entry、capabilities、dependencies、lock hash、source hash、byte size 和 source cache section |
| `VnPolicyBundleSourceCache` | `DONE` | 保存官方 policy source 的包内缓存；release gate 校验 hash/size/entry，report 不输出 source payload |
| `VnExtensionManifest` | `DONE` | 保存 VN extension point 到 provider 的显式绑定和 required capabilities；已覆盖真实 cdylib provider fixture build/load/unload |
| `StoryManifest` / `VariableManifest` / `CommandManifest` / `SystemStoryManifest` | `DONE` | 随 `CompiledStory` 输出 story/state、变量域 key、command/source 绑定和 system page entry；完整 grammar 负例仍由 `S3-SCRIPT-01` 跟踪 |
| `StageModel` | `IN_PROGRESS` | 保存 headless presentation 的 viewport、camera、layer、text window 和 timeline slice，用于 deterministic hash 和布局诊断 |
| `VnAdvancedPresentationManifest` | `DONE` | 保存 advanced profile 的脱敏 evidence id、story hash 和 timeline id；不保存截图、文本、音频、影片或本地路径 |
| `SystemStoryManifest` / `VnSystemUiProfileManifest` | `DONE` | 记录 title/save/load/config/gallery/replay/voice replay/route chart/backlog/localization preview 的 system story entry、policy binding、save migration、unlock source policy 和 localization coverage，并可输出 blocking diagnostic |
| `EditorVisualMetadata` | `IN_PROGRESS` | Graph node 和 Timeline track 只绑定 command id/source map，不形成第二套 runtime model |

ONNX ModelBundle package section 复用同一个 `AstraContainerHeader + SectionTable[] + SectionPayload[] + FooterHash` 容器。`ai.model_bundle_manifest` 保存模型族、pipeline、license/provenance、fine-tune provenance、redistribution、voice authorization、profile budget、platform targets、VFS mount id、section refs、EP policy 和 runtime fingerprint。模型权重、external data、tokenizer、sampler、scheduler、vocoder、reduced ONNX Runtime、Web runtime adapter 和 custom op sidecar 都作为普通 package-backed VFS content section 存放，可用 `Raw`、`Zstd` 和 `EncryptionDescriptor`。

Bundled、on-demand 和 external 分发只改变 package source，不改变读取接口。Provider 通过 Package reader 和 VFS mount 获取 section ref；Release Gate 校验 mount、hash、codec、encryption、runtime vendor cache 和平台 profile，不允许 Shipping provider 读取 loose file 或绝对路径。

## Migration

每个 schema 使用显式 migrator：

```rust
pub trait SchemaMigrator {
    fn from_version(&self) -> SchemaVersion;
    fn to_version(&self) -> SchemaVersion;
    fn migrate(&self, bytes: &[u8]) -> Result<Vec<u8>, MigrationError>;
}
```

Release Gate 验证 `minimum_supported_version -> current_version` 的迁移链完整。

# Stage 2 Media + Package Work

Stage 2 把 Stage 1 的 Runtime 输出接到资产、Cook、Package、Media provider、Windows/Web platform capability、Provider URI Asset VFS 和 release gate。生产完备度审查已将本 Stage 重开为 `IN_PROGRESS`：Package/VFS 权威校验已完成加固，Cook 批次事务与非 Web renderer/media 恢复、资源生命周期和性能门禁仍在实施。Migration 11 同时重开完整 Headless Platform 完成口径：当前分散的 `ScenarioRunner`、CPU frame、AudioGraph meter 和 Player automation 尚未收束为全功能测试 host。Linux、macOS、iOS、Android 移到 [Stage 6 Platform Completion](stage-6-platform-completion.md)。legacy pack reader 仍按 Stage 5 建设，不是本轮完成前置。

## S2-ASSET-01 AssetId、VFS 基础与 sidecar schema

**ID:** `S2-ASSET-01`

**Goal:** `astra-asset` 提供 AssetId、基础 source path policy 和 asset sidecar schema。package 内资产真源迁到 `S2-VFS-01` 的 `asset.vfs_manifest`/`asset.catalog`。

**Depends On:** `S1-CORE-01`、`Docs/modules/asset-pipeline.md`

**Target Paths:** `Engine/Source/Runtime/astra-asset/src/id.rs`、`Engine/Source/Runtime/astra-asset/src/registry.rs`、`Engine/Source/Runtime/astra-asset/src/sidecar.rs`、`Engine/Source/Runtime/astra-asset/tests/sidecar_schema.rs`

**Steps:**

1. 定义 `asset:/` URI、VFS path normalization 和 source path policy。
2. 定义 sidecar Rust 类型，包含 schema、id、source、type、license、importer、cook 和 review。
3. 实现 sidecar validation：缺失 license、非法 source、重复 AssetId 都输出 blocking diagnostic。
4. 编写 YAML roundtrip 和 invalid sidecar 测试。

**Done Evidence:** `cargo test -p astra-asset sidecar_schema` 覆盖有效样例、缺失字段、重复 id 和非法路径。

**Linked Test IDs:** `T-S2-ASSET-01`

## S2-ASSET-02 Import 与 Cook processor

**ID:** `S2-ASSET-02`

**Goal:** `astra-cook` 提供 Importer、CookProcessor、DDC key 和 cook audit。

**Depends On:** `S2-ASSET-01`

**Target Paths:** `Engine/Source/Runtime/astra-asset/src/sidecar.rs`、`Engine/Source/Developer/astra-cook/src/importer.rs`、`Engine/Source/Developer/astra-cook/src/cook.rs`、`Engine/Source/Developer/astra-cook/src/batch.rs`、`Engine/Source/Developer/astra-cook/src/audit.rs`、`Engine/Source/Developer/astra-cook/tests/import_cook.rs`、`Engine/Source/Developer/astra-cook/tests/batch_cook.rs`

**Steps:**

1. 定义 ImportRequest、ImportAudit、CookRequest、CookArtifact 和 DDC key。
2. 实现 source hash、sidecar hash、processor version 和 target profile 共同组成 cache key。
3. 建 Stage 2 image/font/audio metadata importer，不写商业 payload 到测试仓库。
4. 编写 stale artifact、license blocked 和 cook artifact hash 测试。
5. sidecar dependency 构建唯一无环 graph；缺依赖、自依赖、重复依赖和 cycle 阻断。
6. 通过显式 node/byte/concurrency limits 做 bounded parallel cook，支持进程信号取消并隔离 processor panic。
7. 内容寻址 cache 命中必须重新验证 artifact identity；corruption/version/source drift 阻断，不能静默 recook。
8. CLI cook 和 package 文件通过 staging + swap 原子提交；失败或取消不得覆盖上一份完整产物。

**Done Evidence:** `cargo test -p astra-cook --all-targets` 覆盖 fresh/stale/blocked、graph 错误、processor drift/panic、预取消/执行中取消、cache hit/corruption、128 个节点、8 MiB payload、显式容量上限和目录原子替换；`cargo test -p astra-cli --test target_platform nativevn_sample_cooks_packages_validates_and_runs_full_playthrough` 覆盖 `astra.cook_manifest.v2`、真实项目二次 cache hit、旧输出清理和失败 recook 保留完整产物。

**Linked Test IDs:** `T-S2-ASSET-02`

## S2-PACKAGE-01 Binary package writer/reader

**ID:** `S2-PACKAGE-01`

**Goal:** `astra-package` 复用 Stage 1 container，写入 cooked assets、compiled IR、schema registry、provider policy 和 scenario refs。

**Depends On:** `S1-SAVE-01`、`S2-ASSET-02`

**Target Paths:** `Engine/Source/Runtime/astra-package/src/container.rs`、`Engine/Source/Runtime/astra-package/src/builder.rs`、`Engine/Source/Runtime/astra-package/src/reader.rs`、`Engine/Source/Runtime/astra-package/tests/package_roundtrip.rs`

**Steps:**

1. 抽出 save/package 共享 container 类型，避免两套 header 逻辑。
2. 定义 package section ids、section hash、offset、length 和 codec metadata。
3. 实现 package builder，把 cooked artifact、schema registry、module fingerprint 和 scenario refs 写入 section。
4. 实现 streaming reader，只暴露 bounded read API。
5. 编写 package roundtrip、footer hash mismatch 和 section bounds 测试。

**Done Evidence:** `cargo test -p astra-package package_roundtrip` 验证 hash、section bounds、Zstd codec、crypto descriptor 和 schema registry；Runtime save 已改用同一 container。

**Linked Test IDs:** `T-S2-PACKAGE-01`

## S2-VFS-01 Unified Asset VFS mount family

**ID:** `S2-VFS-01`

**Status:** `DONE`

**Goal:** `astra-asset` 成为 VFS contract owner，统一 `provider:/path/file` URI、prefix registry、package mount、local authorized capability、legacy pack backend declaration 和 overlay mount；`astra-package` 只作为 package-backed source。

**Depends On:** `S2-ASSET-01`、`S2-PACKAGE-01`、[Asset VFS Contract](../../contracts/asset-vfs.md)、[Asset VFS Blueprint](../../implementation/asset-vfs.md)

**Target Paths:** `Engine/Source/Runtime/astra-asset/src/vfs.rs`、`Engine/Source/Runtime/astra-asset/tests/vfs_uri.rs`、`Engine/Source/Runtime/astra-package/tests/package_vfs_mount.rs`、`Engine/Source/Runtime/astra-plugin/tests/vfs_provider_registry.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs`

**Steps:**

1. 定义 `VfsUri`、`VfsPrefixDescriptor`、`VfsLayerDescriptor`、`VfsManifestEntry`、`VfsWhiteoutEntry`、`VfsManifest`、`AssetCatalog` 和 redaction rule。
2. 将 package writer 改为写入 `asset.vfs_manifest` 和 `asset.catalog`，并阻断旧 `asset.registry`。
3. 定义单一 `vfs_provider` slot 约定；同 slot 允许多个 provider，由 manifest prefix 选择 `provider_id`。
4. 实现固定 priority overlay 解析、whiteout allowlist、local authorized root host capability、bounds/hash validation 和 URI/path normalization。
5. Release Gate 校验 URI、prefix/provider/capability、package section bounds/hash、overlay whiteout、catalog 引用和 payload/path redaction。

**Done Evidence:** `cargo test -p astra-asset vfs_uri`、`cargo test -p astra-asset vfs_overlayfs`、`cargo test -p astra-package package_vfs_mount`、`cargo test -p astra-package package_roundtrip`、`cargo test -p astra-plugin vfs_provider_registry`、`cargo test -p astra-release vfs_mount_gate` 和 `cargo test -p astra-cli --test target_platform tsuinosora_synthetic_gate_runs_internal_and_patch_player_routes` 通过；report 覆盖 `vfs.uri_format`、`vfs.prefix_registry`、`vfs.package_mount`、`vfs.overlay_mount`、`vfs.catalog`、旧 `asset.registry` blocking、path leak blocking 和 payload leak blocking。legacy pack reader 实现仍留 Stage 5。

**Linked Test IDs:** `T-S2-VFS-01`

## S2-MEDIA-01 Renderer2D slot 与 headless capture

**ID:** `S2-MEDIA-01`

**Goal:** 建立 Renderer2D provider slot、wgpu provider 边界和 headless capture provider。

**Depends On:** `S1-PLUGIN-01`、`Docs/contracts/media.md`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/renderer2d.rs`、`Engine/Source/Runtime/astra-media/tests/headless_capture.rs`

**Steps:**

1. 定义 RendererDescriptor、RendererCreateRequest、Renderer2DProvider 和 render target capability。
2. 只在 provider 内部处理 wgpu/platform handle，不穿过 public API。
3. 实现 headless capture provider，输出 deterministic image hash。
4. 编写 provider eligibility、headless render command 和 hash repeatability 测试。

**Done Evidence:** `cargo test -p astra-media headless_capture` 证明 headless capture hash 可重复，provider descriptor 可被 release gate 检查。

**Linked Test IDs:** `T-S2-MEDIA-01`

## S2-MEDIA-02 TextLayout provider

**ID:** `S2-MEDIA-02`

**Goal:** 建立 TextLayout contract，覆盖 CJK、ruby、inline wait、voice replay metadata 和 backlog shaping。

**Depends On:** `S2-MEDIA-01`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/text_layout.rs`、`Engine/Source/Runtime/astra-media/tests/text_layout.rs`

**Steps:**

1. 定义 TextLayoutRequest、TextRun、RubySpan、LayoutBox 和 VoiceReplayRef。
2. 接入 cosmic-text/Swash provider 边界，平台 font fallback 只通过 capability 报告暴露。
3. 实现 headless layout hash，避免截图作为唯一证据。
4. 编写 CJK shaping、ruby span、line wrap 和 missing font diagnostic 测试。

**Done Evidence:** `cargo test -p astra-media text_layout` 覆盖 CJK、ruby、wrapping、voice replay metadata 和 missing font diagnostic。

**Linked Test IDs:** `T-S2-MEDIA-02`

## S2-MEDIA-03 AudioGraph 与 headless meter

**ID:** `S2-MEDIA-03`

**Goal:** AudioGraph 覆盖 bus、voice、BGM、SE、fade、loop、latency 和 headless meter。

**Depends On:** `S1-RUNTIME-03`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/audio_graph.rs`、`Engine/Source/Runtime/astra-media/tests/audio_graph.rs`

**Steps:**

1. 定义 AudioCommand、AudioGraph source、bus、voice handle ref 和 deterministic meter output。
2. 分离平台 audio output provider 和 headless meter provider。
3. 把 audio wait/fade/loop 完成事件接入 AwaitToken。
4. 编写 bus mix、fade completion、loop marker 和 headless meter hash 测试。

**Done Evidence:** `cargo test -p astra-media audio_graph` 覆盖 bus mix、fade completion、loop marker 和 headless meter hash。

**Linked Test IDs:** `T-S2-MEDIA-03`

## S2-MEDIA-04 FilterGraph typed node validation

**ID:** `S2-MEDIA-04`

**Goal:** FilterGraph 支持 typed node、target、params schema、determinism、fallback 和 release gate rule。

**Depends On:** `S2-MEDIA-01`

**Target Paths:** `Engine/Source/Runtime/astra-media-core/src/filter_graph.rs`、`Engine/Source/Runtime/astra-media/src/filter_graph.rs`、`Engine/Source/Runtime/astra-media/tests/filter_graph.rs`

**Steps:**

1. 定义 FilterGraph source schema、target enum、node id、input/output 和 params。
2. 实现 node provider capability 和 CPU/GPU fallback 选择。
3. 校验环路、缺失 target、参数类型错误和 provider ineligible。
4. 编写 typed validation 和 fallback diagnostic 测试。

**Done Evidence:** `cargo test -p astra-media --test filter_graph` 覆盖 typed validation、fallback diagnostic 和 deterministic CPU filter execution。

**Linked Test IDs:** `T-S2-MEDIA-04`

## S2-MEDIA-05 DecodeProvider 与 fallback policy

**ID:** `S2-MEDIA-05`

**Goal:** 建立 image/audio/video DecodeProvider slot，平台解码优先，桌面 FFmpeg fallback 通过 policy 开关。

**Depends On:** `S1-PLUGIN-01`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/decode.rs`、`Engine/Source/Runtime/astra-media/tests/decode_provider.rs`

**Steps:**

1. 定义 DecodeRequest、DecodeResult、MediaSurfaceToken 和 provider capability。
2. 实现 provider selection：platform provider 优先，fallback provider 只在 profile 允许时启用。
3. public API 只返回 CPU buffer 或 MediaSurfaceToken，不暴露 native handle。
4. 编写 unsupported codec、fallback disabled 和 fallback selected 测试。

**Done Evidence:** `cargo test -p astra-media decode_provider` 证明 provider 选择和 release profile 绑定，而不是按加载顺序抢占；`Engine/Fixtures/PublicDomainMedia/manifest.json` 校验 CC0 fixture 的 sha256、byte size 和 codec metadata；Windows WMF provider 用 `t-rex-roar.mp3` 解码 bounded PCM CPU buffer，用 `flower.mp4` 解码 BGRA 首帧 CPU buffer，视频失败返回 blocking diagnostic；FFmpeg 由 optional feature 显式接入。

**Linked Test IDs:** `T-S2-MEDIA-05`

## S2-GATE-01 Package validate 与 release report

**ID:** `S2-GATE-01`

**Goal:** `astra package validate` 输出 `astra.release_report.v1`，覆盖 package、provider、media 和 scenario refs。

**Depends On:** `S2-PACKAGE-01`、`S2-MEDIA-01`、`S2-MEDIA-05`

**Target Paths:** `Engine/Source/Programs/astra-cli/src/main.rs`、`Engine/Source/Developer/astra-release/src/lib.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs`

**Steps:**

1. 定义 release report Rust 类型和 YAML/JSON 输出。
2. 校验 package integrity、schema migration、provider fingerprint、media decode、scenario refs 和 release package 的 `compiled.project` cook/project 来源。
3. 实现 `astra package validate target/nativevn.astrapkg --profile desktop-release`。
4. 编写 pass、warning、blocked report schema 测试。

**Done Evidence:** `cargo test -p astra-release release_report` 和 `astra package validate target/nativevn.astrapkg --profile desktop-release --report target/release_report.yaml` 输出可机器读取的 `astra.release_report.v1`；`desktop-release`/`web-release` 缺 `compiled.project` 时给出 `ASTRA_PACKAGE_COOKED_PROJECT_MISSING` blocking diagnostic，避免 fixture package 冒充 release 输入。

**Linked Test IDs:** `T-S2-GATE-01`

## S2-PLUGIN-GATE-01 Plugin registry package and release gate

**ID:** `S2-PLUGIN-GATE-01`

**Goal:** Package 写入 Stage 1 产出的 plugin extension registry 和 dependency graph，Release Gate 校验 provider binding、packaged eligibility、conflict 和依赖闭包。

**Depends On:** `S1-PLUGIN-03`、`S2-PACKAGE-01`、`S2-GATE-01`

**Target Paths:** `Engine/Source/Runtime/astra-package/src/builder.rs`、`Engine/Source/Developer/astra-release/src/lib.rs`、`Engine/Source/Runtime/astra-package/tests/package_roundtrip.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs`

**Steps:**

1. `PackageBuildRequest` 写入 `plugin.extension_registry` 和 `plugin.dependency_graph` section。
2. 默认 provider policy 显式绑定 provider，不按加载顺序选择。
3. Release Gate 校验 registry JSON、provider policy binding、packaged eligibility 和 unresolved conflict。
4. Release Gate 校验 required dependency 是否 resolved。
5. 编写 package section、registry pass、conflict blocked、missing binding blocked 和 unresolved dependency blocked 测试。

**Done Evidence:** `cargo test -p astra-package package_roundtrip` 和 `cargo test -p astra-release release_report` 通过；release report 输出 `plugin.extension_registry` 和 `plugin.dependency_graph` evidence。

**Linked Test IDs:** `T-S2-PLUGIN-GATE-01`

## S2-RUNTIME-FSM-01 Product flat StateMachine runtime

**ID:** `S2-RUNTIME-FSM-01`

**Status:** `DONE`

**Goal:** 保持 flat FSM，不引入层级、并行或 pushdown stack；补齐 validation、terminal state、completed 标记、transition priority、冲突诊断、source ref trace 和无外部事件的 deterministic runtime trigger。

**Depends On:** `S1-RUNTIME-02`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/state_machine.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/state_machine_tick.rs`

**Steps:**

1. `validate_state_machine` 输出 `StateMachineValidationReport`，阻断缺失 state、重复 state、未知 transition endpoint 和同 priority guard 冲突。
2. `RuntimeWorld::add_state_machine` 返回 `Result<(), RuntimeError>`，调用方不能忽略 invalid definition。
3. `StateDefinition.terminal` 标记 terminal state，transition commit 后写入 `completed`，后续 tick 不重复执行。
4. `TransitionDefinition.priority` 参与 deterministic 选择；`GuardExpr::Always` 可在固定 tick 边界触发，不依赖外部事件伪造。
5. action 缺失或失败时保留 source ref diagnostic，回滚候选 mutation，不影响其他 state machine。

**Done Evidence:** `cargo test -p astra-runtime --test state_machine_tick` 覆盖 validation、terminal/completed、Always tick trigger、transition action order 和 failure isolation。

**Linked Test IDs:** `T-S2-RUNTIME-FSM-01`

## S2-RUNTIME-AWAIT-01 Await/Fence materialization

**ID:** `S2-RUNTIME-AWAIT-01`

**Status:** `DONE`

**Goal:** AwaitToken 在固定 tick 边界 materialize result；timeout、unknown result、duplicate result、pending token save/load/replay 都有可验证语义。presentation/audio/movie fence 只通过 AwaitToken result 进入 Runtime event queue。

**Depends On:** `S1-RUNTIME-03`、`S1-SAVE-01`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/await_token.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/await_token.rs`、`Engine/Source/Runtime/astra-runtime/tests/save_replay.rs`

**Steps:**

1. `AwaitQueue` 保留 pending、completed 和 diagnostic list，result 只在 `TickInput.fixed_step` 到达后进入 event queue。
2. `deterministic_timeout_step` 到期时生成 `await.timeout` result，按 token id 排序。
3. unknown token result 输出 `ASTRA_AWAIT_RESULT_UNKNOWN` warning，duplicate sequence 输出 `ASTRA_AWAIT_RESULT_DUPLICATE` warning，并阻止重复 event。
4. Pending await token 随 Runtime save/load 保存，replay 只消费记录过的 result 或 deterministic timeout。
5. media fence 继续保持 provider DTO，不把 native presentation/audio/movie handle 写入 deterministic state。

**Done Evidence:** `cargo test -p astra-runtime --test await_token` 和 `cargo test -p astra-runtime save_replay` 覆盖 result ordering、timeout materialization、unknown/duplicate diagnostics 和 pending token serialization。

**Linked Test IDs:** `T-S2-RUNTIME-AWAIT-01`

## S2-SCENARIO-GATE-01 Strict package scenario runner

**ID:** `S2-SCENARIO-GATE-01`

**Status:** `DONE`

**Goal:** `astra test run` 不能把 AstraVN Stage 3 action/assertion 当作 Stage 1 native smoke 通过；package、target、profile、locale 和 scenario refs 进入 report，未知 action/assertion 输出 blocking diagnostic。

**Depends On:** `S1-TEST-01`、`S2-PACKAGE-01`、`S2-TARGET-GATE-01`

**Target Paths:** `Engine/Source/Developer/astra-test/src/scenario.rs`、`Engine/Source/Developer/astra-test/src/runner.rs`、`Engine/Source/Developer/astra-test/src/report.rs`、`Engine/Source/Programs/astra-cli/src/main.rs`

**Steps:**

1. `ScenarioRunOptions` 接收 `package`、`target`、`profile` 和 `headless`，CLI `--package`、`--target` 不再只写日志。
2. Scenario schema 支持顶层 `id`、`package`、`target`、`profile` 和 `locale`，未知顶层字段进入 `ASTRA_SCENARIO_FIELD_UNSUPPORTED`。
3. 未实现的 Stage 3 VN action 和 assertion 进入 `ASTRA_SCENARIO_ACTION_UNSUPPORTED` 或 `ASTRA_SCENARIO_ASSERTION_UNSUPPORTED`，report status 为 `blocked`。
4. package scenario run 读取 package container，校验 `target.manifest` 和 `scenario.refs`；缺 package、缺 target 或 refs 不匹配都阻断。
5. Stage 1 native smoke 仍通过 replay、save/load、plugin fixture 和 delayed event checks。

**Done Evidence:** `cargo test -p astra-test --test native_smoke` 覆盖 native smoke pass、AstraVN sample unsupported action blocked、declared package missing blocked；`cargo test -p astra-cli --test logging` 验证 CLI report/log 分离。

**Linked Test IDs:** `T-S2-SCENARIO-GATE-01`

## Migration 11 Headless Platform 测试后端

[Migration 11](../../migrations/headless-platform-test-backend-migration.md) 已完成文档规划，以下工作项均为 `SPEC_READY`。`S2-MEDIA-01` 与 `S2-MEDIA-03` 保持 `DONE`，只表示局部 renderer/audio contract；九项全部闭合前，Stage 2 Headless 不能恢复完成状态。

| Work ID | Status | Planned boundary | Planned Test ID |
| --- | --- | --- | --- |
| `S2-HEADLESS-CONTRACT-01` | `SPEC_READY` | `HostKind`、`HeadlessHostProfile`、`HostLaunchProfile`，保持六平台 `PlatformId` 与发布 profile v2 | `T-S2-HEADLESS-CONTRACT-01` |
| `S2-HEADLESS-HOST-01` | `SPEC_READY` | `publish = false` 完整 host，覆盖 surface/audio/decode/save/package/input 和 zero-leak lifecycle | `T-S2-HEADLESS-HOST-01` |
| `S2-HEADLESS-MEDIA-01` | `SPEC_READY` | Media-owned reference providers，真实 `SceneCommand`、glyph、FilterGraph、PNG 和 PCM WAV | `T-S2-HEADLESS-MEDIA-01` |
| `S2-HEADLESS-INPUT-01` | `SPEC_READY` | 强类型物理输入、固定时间与双向 JSONL；产品语义直调 blocking | `T-S2-HEADLESS-INPUT-01` |
| `S2-HEADLESS-ARTIFACT-01` | `SPEC_READY` | 全量/检查点/最终/manifest-only retention、显式限额、脱敏 artifact/run report | `T-S2-HEADLESS-ARTIFACT-01` |
| `S2-HEADLESS-CLI-01` | `SPEC_READY` | 独立 `astra-headless run` 与 `serve --stdio` Developer binary | `T-S2-HEADLESS-CLI-01` |
| `S2-HEADLESS-TEST-MIGRATION-01` | `SPEC_READY` | 所有 Runtime test 无例外创建 `HeadlessTestContext`，平台无关测试不保留双轨 | `T-S2-HEADLESS-TEST-MIGRATION-01` |
| `S2-HEADLESS-REVIEW-01` | `SPEC_READY` | 全帧/全音频自动分析、required checkpoint 模型审查、人工容差批准后重跑 | `T-S2-HEADLESS-REVIEW-01` |
| `S2-HEADLESS-PREFLIGHT-01` | `SPEC_READY` | 同 build/package/input 的 Headless preflight，E2 不替代 Windows/Web E3 | `T-S2-HEADLESS-PREFLIGHT-01` |

Planned Done Evidence 只能在实现存在后执行并登记：

```bash
cargo test -p astra-platform-headless
cargo test -p astra-headless
python Tools/run_cargo_isolated.py test --workspace
```

当前没有对应 crate、binary、schema 或 report，以上命令不能写成已通过证据。Migration 11 的实现属于大规模测试基础设施迁移，开工前必须创建新分支。

## S2-PLATFORM-01 Platform capability crate 与分层 probe

**ID:** `S2-PLATFORM-01`

**Status:** `IN_PROGRESS`

**Goal:** `Engine/Source/Platform` 提供共享 `PlatformHost` contract、六个平台 capability crate 和平台 smoke report schema。真实 host 完成按平台分开验收，不再把六个平台 capability crate 编译通过等同于六平台完成。

**Depends On:** `S1-TARGET-01`、`Docs/implementation/platform-host.md`

**Target Paths:** `Engine/Source/Platform/astra-platform/`、`Engine/Source/Platform/astra-platform-windows/`、`Engine/Source/Platform/astra-platform-linux/`、`Engine/Source/Platform/astra-platform-macos/`、`Engine/Source/Platform/astra-platform-ios/`、`Engine/Source/Platform/astra-platform-android/`、`Engine/Source/Platform/astra-platform-web/`

**Steps:**

1. 定义 async `PlatformHostFactory`、`PlatformHostClient`、typed generational handle 与有序 event stream。
2. capability v2 分离 declared、available、selected；只有 live conformance run 能声明 available/selected。
3. host conformance 绑定 build/profile/package/session identity 和完整资源生命周期。
4. 缺 SDK、缺 provider、stale handle、队列溢出、device loss 和 shutdown leak 都显式阻断。
5. Linux、macOS、iOS、Android factory 返回 `PLATFORM_NOT_IMPLEMENTED`，保持 Stage 6。

**Current Evidence:** `cargo test -p astra-platform -p astra-platform-general` 覆盖 contract 和负向门禁。Migration 8 仍等待 Windows/Chrome 同 commit 的完整 conformance 与 Player automation evidence。

**Linked Test IDs:** `T-S2-PLATFORM-01`

## S2-WINDOWS-HOST-01 Windows host probe 与 windowed smoke

**ID:** `S2-WINDOWS-HOST-01`

**Status:** `IN_PROGRESS`

**Goal:** Windows probe 输出真实 SDK、短生命周期 hidden window、wgpu surface + adapter、DPI、IME、input pipe、gamepad capability、WMF audio/video fixture decode、WASAPI stream、known-folder write/read/delete 和 SDK 状态。

**Depends On:** `S2-PLATFORM-01`

**Target Paths:** `Engine/Source/Platform/astra-platform-windows/src/lib.rs`、`Engine/Source/Platform/astra-platform/src/lib.rs`

**Steps:**

1. 使用 winit 创建隐藏短生命周期窗口，记录窗口尺寸、DPI scale、IME enable 和输入事件循环可用性。
2. 使用 XInput probe gamepad capability；无手柄连接不阻断。
3. 用同一个 hidden window 创建 wgpu surface 并请求 compatible adapter，记录 backend、adapter type 和 format count。
4. 使用 WMF 解码公共 MP3 为 PCM、公共 MP4 为 BGRA 首帧，记录 format、bytes 和 hash。
5. 使用 CPAL/WASAPI 初始化 output stream 并渲染短 silent buffer；无默认设备或 stream 失败为 blocking。
6. 使用 Windows Known Folder API 验证 RoamingAppData save store 的 write/read/delete；报告只写能力状态，不写本地路径。
7. `PlatformCapabilityReport.smoke` 只保存 DTO 状态和 machine-readable evidence，不暴露 native handle。

**Current Evidence:** `cargo test -p astra-platform-windows --features platform-test-driver` 覆盖真实窗口、hardware wgpu present/readback、SendInput typed event、WASAPI callback meter、WMF fixture、atomic save 与 hash-bound package range。用户授权文件、HTTPS range、gamepad axis、Player 全服务接线和正式 conformance report 仍未闭合。

**Linked Test IDs:** `T-S2-WINDOWS-HOST-01`

## S2-WINDOWS-WMF-01 Windows Media Foundation DecodeProvider

**ID:** `S2-WINDOWS-WMF-01`

**Status:** `IN_PROGRESS`

**Goal:** Windows Media Foundation provider 作为一拍式 `DecodeProvider`，audio 输出 bounded PCM CPU buffer，video 输出首帧 BGRA CPU buffer；无法 decode 时返回 blocking diagnostic。

**Depends On:** `S2-MEDIA-05`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/decode.rs`、`Engine/Source/Runtime/astra-media/tests/decode_provider.rs`

**Steps:**

1. 使用 `windows` crate 接入 COM、Media Foundation byte stream 和 `IMFSourceReader`。
2. Audio stream 强制输出 PCM，CPU buffer 受 `MAX_DECODED_AUDIO_BYTES` 限制。
3. Video stream 请求 RGB32/BGRA 首帧，校验 frame size、buffer size 和 `MAX_DECODED_VIDEO_FRAME_BYTES`。
4. public API 仍只返回 `DecodeOutput::CpuBuffer`，不暴露 WMF object、COM pointer 或 native handle。

**Done Evidence:** `cargo test -p astra-media decode_provider` 覆盖 public media manifest integrity、Symphonia MP3、WMF MP3 PCM、WMF MP4 BGRA first frame、invalid video blocking diagnostic、platform-first provider selection 和 fallback policy。

**Linked Test IDs:** `T-S2-WINDOWS-WMF-01`

## S2-WINDOWS-GATE-01 Windows platform evidence 接入 Release Gate

**ID:** `S2-WINDOWS-GATE-01`

**Status:** `DONE`

**Goal:** CLI 和 release report 读取 Windows platform report、windowed smoke、WMF decode evidence 和 save store smoke。缺 SDK、缺 WMF、缺 window smoke 或缺 known-folder smoke 都不能静默通过。

**Depends On:** `S2-WINDOWS-HOST-01`、`S2-WINDOWS-WMF-01`、`S2-TARGET-GATE-01`

**Target Paths:** `Engine/Source/Programs/astra-cli/tests/target_platform.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs`、`Engine/Source/Platform/astra-platform/src/lib.rs`

**Steps:**

1. `validate_capability_report` 对 `sdk_status: present` 的平台检查 required smoke。
2. Windows required smoke 是 `windowed_smoke`、`renderer.wgpu_surface`、`decode.wmf.audio`、`decode.wmf.video_first_frame`、`audio.wasapi` 和 `save.known_folder_rw`。
3. `astra platform probe` JSON 输出 smoke evidence；CLI 测试断言 Windows smoke 为 `pass`。
4. Release report 缺 required smoke 时输出 blocking check。

**Current Evidence:** Release Gate 已读取 capability v2 和 host conformance，并校验 profile/package/build/session continuity。正式 Windows Player automation report 尚未在同 run 闭合，因此保持 `IN_PROGRESS`。

**Linked Test IDs:** `T-S2-WINDOWS-GATE-01`

## S2-WEB-HOST-01 Web host probe and browser smoke

**ID:** `S2-WEB-HOST-01`

**Status:** `IN_PROGRESS`

**Goal:** 补 wasm browser host、真实 renderer context、browser media decode、WebCodecs config probe、WebAudio offline render、IndexedDB/OPFS evidence、Blob/File/fetch package source、worker/visibility resume 和 browser smoke。

**Target Paths:** `Engine/Source/Platform/astra-platform-web/`、`Engine/Source/Runtime/astra-media/src/decode.rs`、`Docs/platforms/web.md`

**Steps:**

1. native host 下 Web probe 始终报告 `sdk_status: missing`，不接受环境变量伪造 browser SDK。
2. wasm32 browser 下异步探测 `window`/`document`、WebGL/WebGPU context、HTML media path、WebCodecs config、OfflineAudioContext、IndexedDB/OPFS、Blob/File/fetch、input、Worker 和 visibility。
3. Web required smoke 是 `browser_smoke`、`renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw` 和 `package.web_source_read`。
4. `WebCodecsDecodeProvider` 只在 wasm32 编译，输出 `MediaSurfaceToken`，不暴露 browser object 或 native handle。
5. `uuid` workspace 依赖不启用随机生成 feature，保证 `astra-core` 和 Web platform crate 可在 `wasm32-unknown-unknown` 编译。
6. browser smoke test 使用 `wasm-bindgen-test`，真实浏览器执行命令见 [Web Platform](../../platforms/web.md)；没有 browser platform report 或缺 required evidence 时 Web release 仍阻断。

**Current Evidence:** Chrome 中 canvas/WebGPU present/readback、WebCodecs VP8 encode→decode、OPFS commit/reload/abort 和无用户手势 WebAudio fail-fast 已真实通过；File/fetch allowlist、typed input/lifecycle 与 AudioWorklet queue 已接入，`astra-player-web` 已校验 package/cooked profile 并呈现。用户手势 audio meter、device/context loss 与完整 Player route 尚未闭合，因此保持 `IN_PROGRESS`。

**Linked Test IDs:** `T-S2-WEB-HOST-01`

## S2-TARGET-GATE-01 Package target manifest 与 platform gate

**ID:** `S2-TARGET-GATE-01`

**Goal:** Package build 写入只含单一 packaged Game 的 `target.manifest`，Release Gate 同时校验 Target 和 Platform report。

**Depends On:** `S1-TARGET-01`、`S2-PLATFORM-01`、`S2-GATE-01`

**Target Paths:** `Engine/Source/Runtime/astra-package/src/builder.rs`、`Engine/Source/Developer/astra-release/src/lib.rs`、`Engine/Source/Programs/astra-cli/src/main.rs`

**Steps:**

1. `PackageBuildRequest` 增加 `target_manifest` section。
2. `astra cook`、`astra package build`、`astra package validate` 接收 `--target`。
3. `astra package build` 要求 package target 与 cooked target 一致，并过滤掉 Editor/Program descriptor。
4. `astra package validate` 可读取 `--platform-report` 并阻断缺 SDK 的真实平台完成。
5. 编写 package section、release report 和 CLI probe 测试。

**Done Evidence:** `cargo test -p astra-package package_roundtrip`、`cargo test -p astra-release release_report` 和 `cargo test -p astra-cli --test target_platform` 通过；platform report 现在包含 required smoke evidence，缺失项会阻断 release check。

**Linked Test IDs:** `T-S2-TARGET-GATE-01`

## 跨 Stage Observability follow-up

Asset/Package/Cook/Release、Media/Platform 生命周期与 failure/fallback 边界纳入 `OBS-CORE-01`。Windows shipping bundle 的 reporter role/hash/self-test、启动握手和 tamper blocker 由 `OBS-CRASH-WIN-01` 跟踪；它不改变 Stage 2 已完成边界。

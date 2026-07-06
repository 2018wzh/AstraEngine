# Stage 2 Media + Package Work

Stage 2 把 Stage 1 的 Runtime 输出接到资产、Cook、Package 和 Media provider。目标是可构建二进制 package、可 headless capture、可验证 release report。本页是 planned target 清单，不表示实现已经存在。

## S2-ASSET-01 AssetId、VFS 与 sidecar schema

**ID:** `S2-ASSET-01`

**Goal:** `astra-asset` 提供 AssetId、VFS、AssetRegistry 和 asset sidecar schema。

**Depends On:** `S1-CORE-01`、`Docs/modules/asset-pipeline.md`

**Target Paths:** `Engine/Source/Runtime/astra-asset/src/id.rs`、`Engine/Source/Runtime/astra-asset/src/registry.rs`、`Engine/Source/Runtime/astra-asset/src/sidecar.rs`、`Engine/Source/Runtime/astra-asset/tests/sidecar_schema.rs` planned target

**Steps:**

1. 定义 `asset:/` URI、VFS path normalization 和 source path policy。
2. 定义 sidecar Rust 类型，包含 schema、id、source、type、license、importer、cook 和 review。
3. 实现 sidecar validation：缺失 license、非法 source、重复 AssetId 都输出 blocking diagnostic。
4. 编写 YAML roundtrip 和 invalid sidecar 测试。

**Done Evidence:** sidecar schema 测试覆盖有效样例、缺失字段、重复 id 和非法路径。

**Linked Test IDs:** `T-S2-ASSET-01`

## S2-ASSET-02 Import 与 Cook processor

**ID:** `S2-ASSET-02`

**Goal:** `astra-cook` 提供 Importer、CookProcessor、DDC key 和 cook audit。

**Depends On:** `S2-ASSET-01`

**Target Paths:** `Engine/Source/Developer/astra-cook/src/importer.rs`、`Engine/Source/Developer/astra-cook/src/cook.rs`、`Engine/Source/Developer/astra-cook/src/audit.rs`、`Engine/Source/Developer/astra-cook/tests/import_cook.rs` planned target

**Steps:**

1. 定义 ImportRequest、ImportAudit、CookRequest、CookArtifact 和 DDC key。
2. 实现 source hash、sidecar hash、processor version 和 target profile 共同组成 cache key。
3. 建 Stage 2 image/font/audio metadata importer，不写商业 payload 到测试仓库。
4. 编写 stale artifact、license blocked 和 cook artifact hash 测试。

**Done Evidence:** cook 测试能区分 fresh、stale、blocked 三种 artifact 状态。

**Linked Test IDs:** `T-S2-ASSET-02`

## S2-PACKAGE-01 Binary package writer/reader

**ID:** `S2-PACKAGE-01`

**Goal:** `astra-package` 复用 Stage 1 container，写入 cooked assets、compiled IR、schema registry、provider policy 和 scenario refs。

**Depends On:** `S1-SAVE-01`、`S2-ASSET-02`

**Target Paths:** `Engine/Source/Runtime/astra-package/src/container.rs`、`Engine/Source/Runtime/astra-package/src/builder.rs`、`Engine/Source/Runtime/astra-package/src/reader.rs`、`Engine/Source/Runtime/astra-package/tests/package_roundtrip.rs` planned target

**Steps:**

1. 抽出 save/package 共享 container 类型，避免两套 header 逻辑。
2. 定义 package section ids、section hash、offset、length 和 codec metadata。
3. 实现 package builder，把 cooked artifact、schema registry、module fingerprint 和 scenario refs 写入 section。
4. 实现 streaming reader，只暴露 bounded read API。
5. 编写 package roundtrip、footer hash mismatch 和 section bounds 测试。

**Done Evidence:** package reader 可验证 hash 和 section bounds，Runtime 不依赖源 YAML 启动。

**Linked Test IDs:** `T-S2-PACKAGE-01`

## S2-MEDIA-01 Renderer2D slot 与 headless capture

**ID:** `S2-MEDIA-01`

**Goal:** 建立 Renderer2D provider slot、wgpu provider 边界和 headless capture provider。

**Depends On:** `S1-PLUGIN-01`、`Docs/contracts/media.md`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/renderer2d.rs`、`Engine/Source/Runtime/astra-media/src/headless.rs`、`Engine/Source/Runtime/astra-media/tests/headless_capture.rs` planned target

**Steps:**

1. 定义 RendererDescriptor、RendererCreateRequest、Renderer2DProvider 和 render target capability。
2. 只在 provider 内部处理 wgpu/platform handle，不穿过 public API。
3. 实现 headless capture provider，输出 deterministic image hash。
4. 编写 provider eligibility、headless render command 和 hash repeatability 测试。

**Done Evidence:** headless capture 在同输入下输出相同 hash，provider descriptor 可被 release gate 检查。

**Linked Test IDs:** `T-S2-MEDIA-01`

## S2-MEDIA-02 TextLayout provider

**ID:** `S2-MEDIA-02`

**Goal:** 建立 TextLayout contract，覆盖 CJK、ruby、inline wait、voice replay metadata 和 backlog shaping。

**Depends On:** `S2-MEDIA-01`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/text_layout.rs`、`Engine/Source/Runtime/astra-media/tests/text_layout.rs` planned target

**Steps:**

1. 定义 TextLayoutRequest、TextRun、RubySpan、LayoutBox 和 VoiceReplayRef。
2. 接入 cosmic-text/Swash provider 边界，平台 font fallback 只通过 capability 报告暴露。
3. 实现 headless layout hash，避免截图作为唯一证据。
4. 编写 CJK shaping、ruby span、line wrap 和 missing font diagnostic 测试。

**Done Evidence:** TextLayout 测试能证明 backlog 与 runtime text key 使用同一 layout contract。

**Linked Test IDs:** `T-S2-MEDIA-02`

## S2-MEDIA-03 AudioGraph 与 headless meter

**ID:** `S2-MEDIA-03`

**Goal:** AudioGraph 覆盖 bus、voice、BGM、SE、fade、loop、latency 和 headless meter。

**Depends On:** `S1-RUNTIME-03`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/audio_graph.rs`、`Engine/Source/Runtime/astra-media/src/audio_provider.rs`、`Engine/Source/Runtime/astra-media/tests/audio_graph.rs` planned target

**Steps:**

1. 定义 AudioCommand、AudioGraph source、bus、voice handle ref 和 deterministic meter output。
2. 分离平台 audio output provider 和 headless meter provider。
3. 把 audio wait/fade/loop 完成事件接入 AwaitToken。
4. 编写 bus mix、fade completion、loop marker 和 headless meter hash 测试。

**Done Evidence:** AudioGraph 不保存剧情状态，音频 fence 通过 AwaitToken 在固定 tick 边界完成。

**Linked Test IDs:** `T-S2-MEDIA-03`

## S2-MEDIA-04 FilterGraph typed node validation

**ID:** `S2-MEDIA-04`

**Goal:** FilterGraph 支持 typed node、target、params schema、determinism、fallback 和 release gate rule。

**Depends On:** `S2-MEDIA-01`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/filter_graph.rs`、`Engine/Source/Runtime/astra-media/tests/filter_graph.rs` planned target

**Steps:**

1. 定义 FilterGraph source schema、target enum、node id、input/output 和 params。
2. 实现 node provider capability 和 CPU/GPU fallback 选择。
3. 校验环路、缺失 target、参数类型错误和 provider ineligible。
4. 编写 typed validation 和 fallback diagnostic 测试。

**Done Evidence:** FilterGraph validator 输出可被 release gate 使用的 blocking/warning diagnostic。

**Linked Test IDs:** `T-S2-MEDIA-04`

## S2-MEDIA-05 DecodeProvider 与 fallback policy

**ID:** `S2-MEDIA-05`

**Goal:** 建立 image/audio/video DecodeProvider slot，平台解码优先，桌面 FFmpeg fallback 通过 policy 开关。

**Depends On:** `S1-PLUGIN-01`

**Target Paths:** `Engine/Source/Runtime/astra-media/src/decode.rs`、`Engine/Source/Runtime/astra-media/tests/decode_provider.rs` planned target

**Steps:**

1. 定义 DecodeRequest、DecodeResult、MediaSurfaceToken 和 provider capability。
2. 实现 provider selection：platform provider 优先，fallback provider 只在 profile 允许时启用。
3. public API 只返回 CPU buffer 或 MediaSurfaceToken，不暴露 native handle。
4. 编写 unsupported codec、fallback disabled 和 fallback selected 测试。

**Done Evidence:** decode 测试能证明 provider 选择和 release profile 绑定，而不是按加载顺序抢占。

**Linked Test IDs:** `T-S2-MEDIA-05`

## S2-GATE-01 Package validate 与 release report

**ID:** `S2-GATE-01`

**Goal:** `astra package validate` 输出 `astra.release_report.v1`，覆盖 package、provider、media 和 scenario refs。

**Depends On:** `S2-PACKAGE-01`、`S2-MEDIA-01`、`S2-MEDIA-05`

**Target Paths:** `Engine/Source/Programs/astra-cli/src/package.rs`、`Engine/Source/Developer/astra-release/src/report.rs`、`Engine/Source/Developer/astra-release/tests/release_report.rs` planned target

**Steps:**

1. 定义 release report Rust 类型和 YAML/JSON 输出。
2. 校验 package integrity、schema migration、provider fingerprint、media decode 和 scenario refs。
3. 实现 `astra package validate target/nativevn.astrapkg --profile desktop-release`。
4. 编写 pass、warning、blocked report schema 测试。

**Done Evidence:** release report 可机器读取，blocking domains 与 `Docs/contracts/release-gate.md` 对齐。

**Linked Test IDs:** `T-S2-GATE-01`

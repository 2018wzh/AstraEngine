# Migration 11：Headless Platform 测试后端迁移

本页同时记录统一 Headless Platform 的实现与验收边界。contract、host、Media/PNG/WAV、完整 FFmpeg 视频帧流、JSONL、Developer CLI、统一测试 lifecycle、review bundle 和 formal preflight gate 已进入同一实现路径。Stage 2 的运行与 CI 验收范围现收束为 Windows native Headless，并保留独立 Windows `ffmpeg-vcpkg` job；Linux/macOS 的本机运行、CI 与 artifact portability evidence 延后到 Stage 6，WASM、iOS 和 Android 不支持 Headless。在 Windows job、具名 review 与真实 Windows/Web linked evidence 实际形成之前，除 contract 外仍保持 `IN_PROGRESS`。局部静态或配置存在不能把 Stage 2 Headless、产品可玩性或真实平台验收标为完成。

Migration 11 归入 Stage 2。`S2-MEDIA-01` 和 `S2-MEDIA-03` 继续保留 `DONE`，它们只证明 renderer/audio contract 与局部 deterministic executor 已存在；完整 Headless Platform 必须等本迁移的 contract、host、媒体、输入、产物、CLI、测试收束、模型审查和真实平台 preflight 全部闭合。

## 迁移目标

新增仅供测试和 Developer 工具使用的 `astra-platform-headless`。它与六个平台 host 共用 `PlatformHostClient`、session、typed handle 和 command/event 生命周期，但不属于发布平台，不进入 shipping Player、Game target、package profile 或 Release Gate 的可发布 provider 集合。

Headless 必须接收序列化物理输入，通过与真实 Player 相同的事件入口推进 Runtime，并把真实画面和音频写入有界文件产物。测试不能再用 state hash 生成颜色块、用矩形变化冒充场景、用静态 meter 冒充音频输出，或用 `advance`、`choose`、`open_system` 等语义命令绕开输入映射。

## 架构边界

`PlatformId` 继续只表示 Windows、Linux、macOS、iOS、Android 和 Web。Headless 使用独立的 host 身份：

```rust
pub enum HostKind {
    Platform(PlatformId),
    Headless,
}

pub enum HostLaunchProfile {
    Platform(PlatformHostProfile),
    Headless(HeadlessHostProfile),
}
```

`PlatformHostProfile` 与 `astra.platform_host_profile.v2` 保持发布 schema，不增加 `headless` platform variant。`HeadlessHostProfile` 使用 `astra.headless_host_profile.v2`，声明 provider binding、`all/checkpoints` render policy、输入协议、双帧预算、产物策略、`max_decode_output_bytes`、`max_video_frames`、package/build identity 和测试权限。v2 是硬切换，不读取 v1。native factory 只接受 `HostLaunchProfile::Platform`；Headless factory 只接受 `HostLaunchProfile::Headless`。类型不匹配必须在 `host.start` 阻断。

`checkpoints` 是默认策略：每一帧都完成 canonical scene 校验、资源生命周期提交和 submitted stream hash；仅首帧、具名 checkpoint 所在帧与 surface 销毁前的最终帧执行 raster/readback。多个 checkpoint 命中同一 sequence 时只 materialize 一次，并把全部 id 写进 `checkpoint_ids`。任何未 rasterize 的中间帧仍可因非法引用、栈不平衡、未知 filter 或预算超限立即阻断。

renderer binding 只有 `cpu_reference` 与 `wgpu_offscreen`。前者禁止 `--gpu`，后者必须带 `--gpu`；不一致、缺硬件、CPU adapter 或错误 backend 都阻断，不做 fallback。WGPU 实例、固定 primary backend 和 adapter 策略位于 `astra-platform-common`，Windows/DX12 显式使用随构建锁定的 static DXC，Linux 使用 Vulkan，macOS 使用 Metal；Web、Android、iOS 不提供该入口。

Release API、shipping target、AstraPlayer 和 cooked `platform.profiles` 只能接受 `PlatformHostProfile`。发现 `HostKind::Headless`、Headless schema、Headless provider id 或 Developer binary role 时必须输出 blocking diagnostic，不能忽略、迁移或替换成默认平台。

## Work Items

### `S2-HEADLESS-CONTRACT-01` Host contract

**Status:** `DONE`

**Goal:** 在不扩展 `PlatformId` 的前提下，让 native 与 Headless 共用 host service contract，同时从类型上隔离发布 profile。

**Planned Steps:**

1. 定义 `HostKind`、`HostLaunchProfile`、`HeadlessHostProfile`、artifact policy、input policy 和资源限额。
2. 调整 `PlatformHostFactory` 启动边界，使 factory 显式校验 launch profile variant。
3. 保留 `PlatformHostProfile v2`；Headless profile、协议和报告分别使用独立 schema。
4. 为错误 variant、未知 schema、Headless 进入 release profile 和 shipping dependency graph 增加 blocking diagnostic。

**Done Evidence:** `cargo test -p astra-platform --test headless_launch_profile` 覆盖 profile variant、schema、identity、provider、transport、artifact limit 和 native factory rejection；`cargo test -p astra-release --test release_report` 覆盖 Headless schema、cooked launch profile 和 shipping target release rejection。

### `S2-HEADLESS-HOST-01` Full host backend

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-CONTRACT-01`

**Goal:** 建立 `publish = false` 的 `astra-platform-headless`，真实执行完整 `PlatformHostClient` 服务面。

Headless 必须实现 window/surface、RGBA present/capture、audio output、decode session、transactional save、bounded package source、event queue、ordered completion、shutdown 和 generational resource lifecycle。stale handle、重复 close、帧或音频 sequence 回退、越界读取、队列溢出、输出目录冲突、未提交 save、未关闭资源和 artifact 写入失败都必须阻断。

`Bundled`、`UserAuthorized` 和 `HttpsRange` package source 都要执行真实 bounded read 与 hash 校验。`HttpsRange` 必须禁用 redirect 和 content encoding，要求服务器返回严格匹配请求与总长度的 `206 Content-Range`；打开 source 时按有界 block 扫描并校验完整 package hash，后续读取重新校验对应 block hash，不能以一次完整 GET 冒充 range source。自动化测试可以使用显式字节流、文件输入或本地 deterministic TLS range fixture。任何 command 都不能返回 synthetic success。Headless 只由 test harness、`astra-test` 和 Developer 工具引用，不进入 shipping dependency graph。

**Planned Done Evidence:** `cargo test -p astra-platform-headless host_services` 覆盖全部 command、负向输入、资源回收和 zero-leak shutdown。

### `S2-HEADLESS-MEDIA-01` Reference media composition

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-HOST-01`、`S2-MEDIA-01`、`S2-MEDIA-02`、`S2-MEDIA-03`、`S2-MEDIA-04`、`S2-MEDIA-05`

**Goal:** 通过显式 provider binding 组合完整 CPU renderer、字体 shaping、AudioGraph mixer 与跨平台 decode provider。

renderer、TextLayout、AudioGraph 和 decode contract 仍由 Media 层持有。Headless host 负责生命周期、输入和文件产物，不把媒体实现塞进 platform contract。输出必须来自真实 `SceneCommand`、glyph、纹理、视频帧、FilterGraph 和 AudioGraph。图像规范为 lossless PNG；音频规范为固定采样率、固定声道布局的 PCM S16LE WAV。

image 与 Symphonia provider 默认可用。视频只在 profile 显式绑定 `ffmpeg-vcpkg` 且 feature/provider probe 成功时启用。Headless 解码完整 timestamped BGRA 帧流，逐帧校验 sequence、PTS、duration、尺寸、hash、总帧数和总字节上限；NativeVN 按固定时间呈现每帧，保存时只记录 asset/hash/cursor，恢复后重新从 package 解码并复核 identity。测试声明需要视频而 provider 不可用时必须阻断，不能退回首帧静态图、`SyntheticPlatformDecodeProvider`、空帧或静态 hash。

Platform decode contract 同时包含 `Image`、`Audio` 与 `Video`。Headless 的 `Image` 路径必须经 `ImageDecodeProvider` 返回真实 RGBA；factory 会逐项核对 renderer、text、mixer、image/audio/video decode、save 与 package binding。未知 binding、未编译的 `ffmpeg-vcpkg` 或 probe 失败均在 session start 阻断，不能声明任意 provider 后仍使用内置实现。

`max_video_frames` 与 `max_decode_output_bytes` 必须从 `HeadlessHostProfile` 经 `ProductOpenRequest` 传入产品媒体宿主，host 与 product contract 使用同一份 policy，不能在 Player 内另设较小的隐式上限。`MovieLoopMode::Loop` 必须保留到 video request、snapshot 和 restore；循环帧以固定逻辑时间重复呈现。用户通过物理 Advance 输入跳过活动视频时，由 product adapter 完成对应 fence，并记录可观测事件；输入协议不暴露 `complete_wait`。

**Planned Done Evidence:** 公开 fixture 的真实图片、音频和视频经过 provider、Player presentation、Headless present/audio output 后生成 PNG/WAV，report 能绑定输入 hash、provider、frame/audio sequence 和 artifact hash。

### `S2-HEADLESS-INPUT-01` Serialized input protocol

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-HOST-01`

**Goal:** 新增平台无关、强类型的 `astra.user_input_sequence.v1` 与双向 `astra.headless_protocol.v1` JSONL 协议。

文件、stdin/stdout 与实时 CLI 使用同一 framing。每条消息必须包含 schema、session id、严格递增 sequence，以及固定 tick 或 time boundary。允许的操作只有 focus/resume、keyboard、IME、pointer、wheel、touch、gamepad、固定时间推进、await、checkpoint 和 shutdown。

await 与 checkpoint 只观察状态、等待条件或请求采样，不能直接提交产品命令。`advance`、`choose`、`open_system`、直接 `VnPlayerCommand`、DOM callback、JS runtime hook 和直接 Runtime state mutation 都必须在 schema validation 或 host adapter 边界阻断。

产品 adapter 可以输出 `media.active_video`、`media.active_voice` 等脱敏布尔 observation，用来把自动化序列同步到真实媒体生命周期。它们只反映状态；视频 skip、文本推进和 choice 仍必须由 keyboard、pointer、touch 或 gamepad 等物理输入触发。

**Planned Done Evidence:** file 与 stdio 输入产生相同 transcript/hash；重复、回退、跨 session、未知事件、非法时间和语义快捷命令全部失败。

### `S2-HEADLESS-ARTIFACT-01` Image/audio artifacts

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-MEDIA-01`、`S2-HEADLESS-INPUT-01`

**Goal:** 输出可供自动比较、模型视觉能力和音频工具直接检查的真实产物。

默认策略为 `all`，保存每次 present 的 PNG 和每个 audio output 的完整 WAV。profile 还可以显式选择 `checkpoints`、`final` 或 `manifest-only`。每次运行必须声明 frame、byte、duration 和 artifact count 上限；超限时立即阻断，不能截断后继续或把缺失产物降为 warning。

`astra.headless_artifact_manifest.v2` 分开记录 submitted scene count/hash 与实际 rasterized RGBA count/hash，并记录 render policy、provider/backend/device/vendor/device id 及脱敏 adapter/driver identity。`astra.headless_run_report.v2` 记录相同双流 identity、build/package/input/profile/session continuity、checks 和 diagnostic。报告不得包含本地绝对路径、截图内容、音频内容、商业正文、secret 或 native handle。

公开 fixture 的 PNG/WAV 与基线可以提交。商业或本地产品产物只能放在 ignored 私有工作区；仓库只保存脱敏 hash、尺寸、时长、指标和 signoff。

### `S2-HEADLESS-CLI-01` Developer binary

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-ARTIFACT-01`

**Goal:** 新增独立的 `astra-headless` Developer binary，不把测试后端接进 shipping Player。

`astra-headless run` 读取 JSONL 文件，写入 artifact 目录、manifest 和 report。`astra-headless serve --stdio` 使用相同 JSONL schema 做双向交互；二者可通过 `--gpu` 启用已在 profile 绑定的 WGPU offscreen。machine-readable 输出只走 stdout，日志只走 stderr。`prepare-review` 从通过校验的稀疏 report/manifest 选择 required checkpoint、首尾帧、最大差异帧、失败邻近帧和完整 WAV，逐个复核文件 hash 后生成 `astra.headless_review_bundle.v2`；`validate-review` 阻断缺 checkpoint verdict 或试图覆盖自动失败的 review；`link-preflight` 只接受与 Headless report 完全同 identity 的 `astra.platform_run_identity.v1`。

Rust 测试优先通过共享 harness 直接创建 session。CLI 面向模型、人工和外部工具。Migration 11 完成后，旧 `astra test run --headless` 必须返回带迁移说明的 blocking diagnostic；不保留隐式 alias，也不静默转发到新 binary。

**Planned Done Evidence:** `cargo test -p astra-headless` 覆盖 file/stdio 等价性、日志与 report 分流、损坏 JSONL、断流、限额和非零退出状态。

### `S2-HEADLESS-TEST-MIGRATION-01` Test convergence

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-CLI-01`

**Goal:** 把所有平台无关 Runtime 测试收束到统一 `HeadlessTestContext`，不保留长期双轨。

`Engine/Source/Runtime` 下每个测试都必须启动并关闭 Headless session，包括 parser、schema、derive 和纯数据测试。静态测试仍可直接断言被测 API，但生命周期必须经过统一 context。会 tick、render、mix、decode、save/load、package read 或消费输入的测试必须从 Headless service/client 路径执行。

Developer、Modules 和 Programs 中的平台无关 Runtime、Player 与 full-flow 测试同步迁入。Windows/Web 等真实平台测试继续使用 native host，但只能在对应 Headless preflight 已通过后启动。迁移清单必须覆盖直接 `HeadlessRendererProvider`、独立 AudioGraph meter、手写 mock sink、ScenarioRunner 私有执行和测试专用产品命令。

**Planned Done Evidence:** `cargo test --workspace` 能证明所有 Runtime test target 创建 Headless session、执行 zero-leak shutdown，并阻断未迁移入口。

### `S2-HEADLESS-REVIEW-01` Automated and model review

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-TEST-MIGRATION-01`

**Goal:** 为产品、Player、样例、full-playthrough 与 migration acceptance 建立自动比较和模型审查双重门禁。

自动分析覆盖全部帧和全部音频。模型必须逐一查看 required checkpoint、首尾帧、最大差异帧和失败邻近帧。音频审查必须读取 WAV，并使用跨平台工具检查波形、频谱、响度、静音、削波、声道和时长；涉及语音可懂度、内容或音画同步时，还需要人工或具备音频理解能力的工具试听。

`astra.headless_review_bundle.v2` 只列出相对 artifact path、hash、选择角色、sequence 与 checkpoint；`astra.headless_review.v2` 记录 run report hash、检查点、工具/模型身份、verdict 和 diagnostic，不记录媒体内容或本地路径。正式 Release Gate 同时校验 bundle、review 和 preflight link，模型不能覆盖自动失败。普通 Runtime 测试只要求自动断言，不要求模型逐项审查。

每个 checkpoint 可以定义图像与音频容差；未声明时使用本 migration 固定的受控宽松默认值。图像比较记录像素差比例、通道差、结构相似度和非空区域；音频比较记录时长、peak/RMS、BS.1770 K-weighting 门限积分响度、覆盖完整时间线的固定 1024-point/50% overlap FFT、静音和削波。超过当前容差的 run 保持失败。任何偏离默认值的容差都必须绑定具名人工 `astra.headless_tolerance_approval.v2`；CLI 校验 approval 文件 hash 与 tolerance-set hash，run report 固化新的 checkpoint config hash。修改后必须完整重跑，旧 report 不得被改写为 pass。

### `S2-HEADLESS-PREFLIGHT-01` Real-platform preflight

**Status:** `IN_PROGRESS`

**Depends On:** `S2-HEADLESS-REVIEW-01`

**Goal:** 所有真实产品平台验收先通过 Headless 用户仿真，再启动 native/browser host。

Headless 与真实平台 run 必须绑定同一 build fingerprint、同一 cooked package hash、同一 input sequence hash，以及相同的 scenario、target 和 content identity。两次运行各自拥有 profile/session id，由 `astra.headless_preflight_link.v2` 显式关联。identity mismatch、Headless blocked、缺模型审查或缺 required artifact 时，真实平台验收不得启动。

Headless 最多形成 E2 平台无关产品证据。它不能替代 Windows/Web E3 的真实输入、窗口/canvas、真实音频设备、host consumed trace、route 和同 run identity，也不能关闭 `player.full_playable`。

## 模型执行指导

模型处理真实产品测试时按以下顺序工作：

1. 在新鲜 checkout/build identity 下运行 `astra-headless`，使用目标平台验收将要使用的同一 cooked package 和 input sequence。
2. 先检查 `astra.headless_run_report.v2`、artifact manifest 和自动比较结果；任何 blocking diagnostic 都必须原样保留。
3. 使用可用的图像工具打开 required checkpoint、首尾帧、最大差异帧和失败邻近帧，检查真实内容、布局、裁剪、空白、资源错误与时序变化。
4. 使用跨平台音频工具读取 WAV、生成波形/频谱并检查响度、静音、削波、声道和时长。需要判断语音内容或同步时执行试听；工具不具备能力时明确要求人工试听，不能凭 meter 猜测。
5. 通过 `prepare-review` 生成不可自行增删的 review bundle，逐项检查后写出脱敏 `astra.headless_review.v2`，再用 `validate-review` 复核。自动检查与模型审查都通过后，真实平台工具才可生成 `astra.platform_run_identity.v1`，并由 `link-preflight` 建立关联。

模型不得上传商业媒体、把媒体内容写入 report、修改失败结果、擅自放宽容差，或用 Headless 结果替代真实平台 evidence。

## 状态与验收

Migration 11 已进入主线实施。`S2-HEADLESS-CONTRACT-01` 保持 `DONE`，其余项保持 `IN_PROGRESS`。Stage 2 只按 Windows native Headless evidence 关闭；Linux/macOS portability 不再阻断这些工作项，但必须在 Stage 6 独立登记和验收。只有对应 Rust 类型、schema、crate、binary、Windows workspace test inventory、artifact、review、shipping graph 和 preflight report 都通过后，才能逐项改为 `DONE`。

受控 library target 统一设置 `doctest = false`，避免 Cargo doctest 绕过 session lifecycle；compile/schema 示例由使用 `#[astra_headless_test::test]` 的受控测试承担。convergence checker会同时核对原始 test attribute、legacy path、library doctest 配置和 inventory。

实现验收必须执行：

```bash
python Tools/check_docs.py
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p astra-headless
cargo test --workspace
cargo build -p astra-headless --features ffmpeg-vcpkg
cargo test --workspace --features ffmpeg-vcpkg
```

workspace test 还必须通过 `astra.headless_test_inventory.v1`、`astra.headless_shipping_graph_report.v1` 和每个 session 的 zero-leak run report 检查。命令或正式 evidence 未通过时，状态不能写成 `DONE`。

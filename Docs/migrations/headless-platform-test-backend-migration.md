# Migration 11：Headless Platform 测试后端迁移

本页定义现有平台无关测试向统一 Headless Platform 后端迁移的实施路线。当前仓库已有 `ScenarioRunner`、`HeadlessRenderer`、AudioGraph meter、`PlatformCommandSink` 和 Player automation contract，但它们仍是分散的局部能力。Migration 11 尚未实施，以下工作项全部为 `SPEC_READY`，不能据此把 Stage 2 Headless、产品可玩性或真实平台验收标为完成。

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

`PlatformHostProfile` 与 `astra.platform_host_profile.v2` 保持发布 schema，不增加 `headless` platform variant。`HeadlessHostProfile` 使用 `astra.headless_host_profile.v1`，声明 provider binding、输入协议、产物策略、资源限额、package/build identity 和测试权限。native factory 只接受 `HostLaunchProfile::Platform`；Headless factory 只接受 `HostLaunchProfile::Headless`。类型不匹配必须在 `host.start` 阻断。

Release API、shipping target、AstraPlayer 和 cooked `platform.profiles` 只能接受 `PlatformHostProfile`。发现 `HostKind::Headless`、Headless schema、Headless provider id 或 Developer binary role 时必须输出 blocking diagnostic，不能忽略、迁移或替换成默认平台。

## Planned Work

### `S2-HEADLESS-CONTRACT-01` Host contract

**Status:** `SPEC_READY`

**Goal:** 在不扩展 `PlatformId` 的前提下，让 native 与 Headless 共用 host service contract，同时从类型上隔离发布 profile。

**Planned Steps:**

1. 定义 `HostKind`、`HostLaunchProfile`、`HeadlessHostProfile`、artifact policy、input policy 和资源限额。
2. 调整 `PlatformHostFactory` 启动边界，使 factory 显式校验 launch profile variant。
3. 保留 `PlatformHostProfile v2`；Headless profile、协议和报告分别使用独立 schema。
4. 为错误 variant、未知 schema、Headless 进入 release profile 和 shipping dependency graph 增加 blocking diagnostic。

**Planned Done Evidence:** `cargo test -p astra-platform headless_launch_profile` 覆盖 profile variant、schema、release rejection 和 session identity。该命令在实现前不是完成证据。

### `S2-HEADLESS-HOST-01` Full host backend

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-CONTRACT-01`

**Goal:** 建立 `publish = false` 的 `astra-platform-headless`，真实执行完整 `PlatformHostClient` 服务面。

Headless 必须实现 window/surface、RGBA present/capture、audio output、decode session、transactional save、bounded package source、event queue、ordered completion、shutdown 和 generational resource lifecycle。stale handle、重复 close、帧或音频 sequence 回退、越界读取、队列溢出、输出目录冲突、未提交 save、未关闭资源和 artifact 写入失败都必须阻断。

`Bundled`、`UserAuthorized` 和 `HttpsRange` package source 都要执行真实 bounded read 与 hash 校验；自动化测试可以使用显式字节流、文件输入或本地 deterministic range fixture。任何 command 都不能返回 synthetic success。Headless 只由 test harness、`astra-test` 和 Developer 工具引用，不进入 shipping dependency graph。

**Planned Done Evidence:** `cargo test -p astra-platform-headless host_services` 覆盖全部 command、负向输入、资源回收和 zero-leak shutdown。

### `S2-HEADLESS-MEDIA-01` Reference media composition

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-HOST-01`、`S2-MEDIA-01`、`S2-MEDIA-02`、`S2-MEDIA-03`、`S2-MEDIA-04`、`S2-MEDIA-05`

**Goal:** 通过显式 provider binding 组合完整 CPU renderer、字体 shaping、AudioGraph mixer 与跨平台 decode provider。

renderer、TextLayout、AudioGraph 和 decode contract 仍由 Media 层持有。Headless host 负责生命周期、输入和文件产物，不把媒体实现塞进 platform contract。输出必须来自真实 `SceneCommand`、glyph、纹理、视频帧、FilterGraph 和 AudioGraph。图像规范为 lossless PNG；音频规范为固定采样率、固定声道布局的 PCM S16LE WAV。

image 与 Symphonia provider 默认可用。视频只在 profile 显式绑定 `ffmpeg-vcpkg` 且 feature/provider probe 成功时启用。测试声明需要视频而 provider 不可用时必须阻断，不能改用 `SyntheticPlatformDecodeProvider`、空帧或静态 hash。

**Planned Done Evidence:** 公开 fixture 的真实图片、音频和视频经过 provider、Player presentation、Headless present/audio output 后生成 PNG/WAV，report 能绑定输入 hash、provider、frame/audio sequence 和 artifact hash。

### `S2-HEADLESS-INPUT-01` Serialized input protocol

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-HOST-01`

**Goal:** 新增平台无关、强类型的 `astra.user_input_sequence.v1` 与双向 `astra.headless_protocol.v1` JSONL 协议。

文件、stdin/stdout 与实时 CLI 使用同一 framing。每条消息必须包含 schema、session id、严格递增 sequence，以及固定 tick 或 time boundary。允许的操作只有 focus/resume、keyboard、IME、pointer、wheel、touch、gamepad、固定时间推进、await、checkpoint 和 shutdown。

await 与 checkpoint 只观察状态、等待条件或请求采样，不能直接提交产品命令。`advance`、`choose`、`open_system`、直接 `VnPlayerCommand`、DOM callback、JS runtime hook 和直接 Runtime state mutation 都必须在 schema validation 或 host adapter 边界阻断。

**Planned Done Evidence:** file 与 stdio 输入产生相同 transcript/hash；重复、回退、跨 session、未知事件、非法时间和语义快捷命令全部失败。

### `S2-HEADLESS-ARTIFACT-01` Image/audio artifacts

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-MEDIA-01`、`S2-HEADLESS-INPUT-01`

**Goal:** 输出可供自动比较、模型视觉能力和音频工具直接检查的真实产物。

默认策略为 `all`，保存每次 present 的 PNG 和每个 audio output 的完整 WAV。profile 还可以显式选择 `checkpoints`、`final` 或 `manifest-only`。每次运行必须声明 frame、byte、duration 和 artifact count 上限；超限时立即阻断，不能截断后继续或把缺失产物降为 warning。

`astra.headless_artifact_manifest.v1` 只记录相对路径、hash、尺寸、色彩空间、采样率、声道、时长、sequence、checkpoint 和 provider identity。`astra.headless_run_report.v1` 记录 build/package/input/profile/session continuity、checks 和 diagnostic。报告不得包含本地绝对路径、截图内容、音频内容、商业正文、secret 或 native handle。

公开 fixture 的 PNG/WAV 与基线可以提交。商业或本地产品产物只能放在 ignored 私有工作区；仓库只保存脱敏 hash、尺寸、时长、指标和 signoff。

### `S2-HEADLESS-CLI-01` Developer binary

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-ARTIFACT-01`

**Goal:** 新增独立的 `astra-headless` Developer binary，不把测试后端接进 shipping Player。

`astra-headless run` 读取 JSONL 文件，写入 artifact 目录、manifest 和 report。`astra-headless serve --stdio` 使用相同 JSONL schema 做双向交互；machine-readable 输出只走 stdout，日志只走 stderr。

Rust 测试优先通过共享 harness 直接创建 session。CLI 面向模型、人工和外部工具。Migration 11 完成后，旧 `astra test run --headless` 必须返回带迁移说明的 blocking diagnostic；不保留隐式 alias，也不静默转发到新 binary。

**Planned Done Evidence:** `cargo test -p astra-headless` 覆盖 file/stdio 等价性、日志与 report 分流、损坏 JSONL、断流、限额和非零退出状态。

### `S2-HEADLESS-TEST-MIGRATION-01` Test convergence

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-CLI-01`

**Goal:** 把所有平台无关 Runtime 测试收束到统一 `HeadlessTestContext`，不保留长期双轨。

`Engine/Source/Runtime` 下每个测试都必须启动并关闭 Headless session，包括 parser、schema、derive 和纯数据测试。静态测试仍可直接断言被测 API，但生命周期必须经过统一 context。会 tick、render、mix、decode、save/load、package read 或消费输入的测试必须从 Headless service/client 路径执行。

Developer、Modules 和 Programs 中的平台无关 Runtime、Player 与 full-flow 测试同步迁入。Windows/Web 等真实平台测试继续使用 native host，但只能在对应 Headless preflight 已通过后启动。迁移清单必须覆盖直接 `HeadlessRendererProvider`、独立 AudioGraph meter、手写 mock sink、ScenarioRunner 私有执行和测试专用产品命令。

**Planned Done Evidence:** checkout-bound `test --workspace` 能证明所有 Runtime test target 创建 Headless session、执行 zero-leak shutdown，并阻断未迁移入口。

### `S2-HEADLESS-REVIEW-01` Automated and model review

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-TEST-MIGRATION-01`

**Goal:** 为产品、Player、样例、full-playthrough 与 migration acceptance 建立自动比较和模型审查双重门禁。

自动分析覆盖全部帧和全部音频。模型必须逐一查看 required checkpoint、首尾帧、最大差异帧和失败邻近帧。音频审查必须读取 WAV，并使用跨平台工具检查波形、频谱、响度、静音、削波、声道和时长；涉及语音可懂度、内容或音画同步时，还需要人工或具备音频理解能力的工具试听。

`astra.headless_review.v1` 记录 artifact hash、检查点、工具/模型身份、verdict 和 diagnostic，不记录媒体内容或本地路径。模型不能覆盖自动失败。普通 Runtime 测试只要求自动断言，不要求模型逐项审查。

每个 checkpoint 可以定义图像与音频容差；未声明时按 exact 处理。图像比较记录像素差比例、通道差、结构相似度和非空区域；音频比较记录时长、peak/RMS、响度、频谱、静音和削波。超过当前容差的 run 保持失败。模型只能提出调整建议；只有人工批准后才能修改配置、生成新的 approval/config hash，并完整重跑。旧 report 不得被改写为 pass。

### `S2-HEADLESS-PREFLIGHT-01` Real-platform preflight

**Status:** `SPEC_READY`

**Depends On:** `S2-HEADLESS-REVIEW-01`

**Goal:** 所有真实产品平台验收先通过 Headless 用户仿真，再启动 native/browser host。

Headless 与真实平台 run 必须绑定同一 build fingerprint、同一 cooked package hash、同一 input sequence hash，以及相同的 scenario、target 和 content identity。两次运行各自拥有 profile/session id，由 `astra.headless_preflight_link.v1` 显式关联。identity mismatch、Headless blocked、缺模型审查或缺 required artifact 时，真实平台验收不得启动。

Headless 最多形成 E2 平台无关产品证据。它不能替代 Windows/Web E3 的真实输入、窗口/canvas、真实音频设备、host consumed trace、route 和同 run identity，也不能关闭 `player.full_playable`。

## 模型执行指导

模型处理真实产品测试时按以下顺序工作：

1. 在新鲜 checkout/build identity 下运行 `astra-headless`，使用目标平台验收将要使用的同一 cooked package 和 input sequence。
2. 先检查 `astra.headless_run_report.v1`、artifact manifest 和自动比较结果；任何 blocking diagnostic 都必须原样保留。
3. 使用可用的图像工具打开 required checkpoint、首尾帧、最大差异帧和失败邻近帧，检查真实内容、布局、裁剪、空白、资源错误与时序变化。
4. 使用跨平台音频工具读取 WAV、生成波形/频谱并检查响度、静音、削波、声道和时长。需要判断语音内容或同步时执行试听；工具不具备能力时明确要求人工试听，不能凭 meter 猜测。
5. 写出脱敏 `astra.headless_review.v1`。自动检查与模型审查都通过后，才允许生成 preflight link 并启动真实平台验收。

模型不得上传商业媒体、把媒体内容写入 report、修改失败结果、擅自放宽容差，或用 Headless 结果替代真实平台 evidence。

## 状态与验收

Migration 11 的实现必须在新分支完成。文档规划完成后，所有 `S2-HEADLESS-*` 仍保持 `SPEC_READY`。只有对应 Rust 类型、schema、crate、binary、workspace test migration、artifact、review 和 preflight report 都通过后，才能逐项改为 `DONE`。

本轮文档验收只有：

```bash
python Tools/check_docs.py
```

未来实现完成后，Planned Done Evidence 至少包括：

```bash
cargo test -p astra-platform-headless
cargo test -p astra-headless
python Tools/run_cargo_isolated.py test --workspace
```

这些命令在实际实现和报告存在前不能写成已通过证据。

# Stage 5 AstraEMU Work

Stage 5 实现旧 VN 兼容与现代化套件。AstraEMU Manager 自身仍是 Program target；被启动的 legacy case 通过 `AstraEmuRuntimeProvider` 运行，provider 为每个 session 创建独立 `RuntimeWorld`。legacy family 只注册 `LegacyRuntimeProvider` facade，私有 VM、VFS、媒体状态、诊断和 snapshot section 都留在 provider session 内。Manager/RuntimeWorld 持有统一管理、Trusted Luau、文本翻译和滤镜 preset，不把这些职责塞进 family VM public API。family API、FVP provider、Manager Core、ECNU translation、Slint host、desktop/Android dynamic registration、iOS static registry、签名工具和 evidence encoder 已进入主 workspace。rfvp VM/parser differential golden 已绑定官方 `0.4.0` commit；Windows E3 正式验收按当前实施方向暂缓，正式平台签名、完整 media/full-flow parity、Windows/Android E3 与真机证据仍未形成，因此 Stage 5 继续保持 `IN_PROGRESS`。

FVP host-command media 已覆盖资源引用音频、流式 PCM、WMV/MPEG 与 Windows MP4 影片、fixed-tick frame selection、同 device wgpu composition、严格 `MediaFence` identity，以及 pending movie 的 family snapshot/rebind。runtime snapshot 使用有界压缩 envelope，并嵌入 live texture 的精确 RGBA；host-command audio restore 不重新读取 raw desktop VFS，而由 host 清理旧 stream 后通过 session resource channel 重建。Windows 本机授权样本的 ignored Headless run 已连续执行 240 tick、240 个 presented frame和一次 save/restore continuation，lifecycle 与 redaction report 通过；两个 checkpoint 的 frame hash 相同，且该 run 属于 Headless E2，所以不能据此声明输入造成视觉变化或 Windows E3。同一样本也通过签名 development package 启动真实 Slint/WGPU 单窗口并保持响应，随后从 window close 走完 Manager/CLI shutdown；本轮修复了无 extension audio 的 container sniff、无 active voice 的 silent callback 误报，以及丢弃中间 texture delta 导致 draw 引用缺失。该次 native run 没有绑定自动输入、视觉变化、非静音 meter、route/terminal 和正式 run identity，仍只是 Windows E3 子链，不能关闭 E3。WMV/MPEG 使用增量 packet decoder；Windows MP4 video/audio 分别使用 stateful WMF SourceReader，按 PTS 合并后进入 16-frame/500ms 预取 ring 和可裁剪 PCM stream，并执行 running frame/byte/sample/timestamp budget。没有 public sanitized full-flow movie fixture 和真实 Windows/Android run identity，不能据此提升为 E3。

## S5-GAME-RUNTIME-01 AstraEmuRuntimeProvider gameplay runtime

**ID:** `S5-GAME-RUNTIME-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU 作为 `AstraEmuRuntimeProvider` 与 `NativeVnRuntimeProvider`、后续 `AstraRpgRuntimeProvider` 同级接入，不直接替换 `RuntimeWorld`。

**Depends On:** `S2-VFS-01`、`S3-RUNTIME-PROVIDER-01`、[Game Runtime Provider Contract](../../contracts/game-runtime-provider.md)、[Game Runtime Provider Blueprint](../../implementation/game-runtime-provider.md)

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager-core/src/runtime_provider.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/main.rs`

**Steps:**

1. 定义 `AstraEmuRuntimeProvider` descriptor、prepare/probe/open/step/save/restore/shutdown、package section plan、release checks 和 editor metadata。
2. 让 case target 显式绑定 `astra_emu` runtime provider；Manager 只负责 program shell、profile、UI 和 local operator workflow。
3. `open` 创建 RuntimeWorld lifecycle StateMachine，并选择 family `LegacyRuntimeProvider` session。
4. `step` 调用 family provider，收集 `LegacyEffect`、AwaitToken、TextCaptureEvent、PresentationCommand、AudioCommand、trace 和 diagnostic。
5. Release Gate 校验 `emu.game_runtime_provider`、provider fingerprint、package sections、save/replay hash 和 report redaction。

**Done Evidence:** `cargo test -p astra-emu-manager game_runtime_provider` 和 `cargo test -p astra-release emu_gate` 通过；report 输出 `emu.game_runtime_provider`，且 family plugin 仍不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

**Linked Test IDs:** `T-S5-GAME-RUNTIME-01`

## S5-EMUCORE-SM-01 EmulatorCore VM state-machine mapping

**ID:** `S5-EMUCORE-SM-01`

**Status:** `IN_PROGRESS`

**Goal:** Family 内部把旧 VM 映射为私有 scheduler、context、basic-block 和 action 状态机，公共 Runtime 只接收可序列化 effect 和 snapshot envelope。

**Depends On:** `S5-GAME-RUNTIME-01`、`S5-FAMILY-01`、[EmulatorCore StateMachine Mapping](../../implementation/emulator-core-state-machine.md)

**Target Paths:** `AstraEMU/Source/FamilyApi/astra-emu-family-api/src/lib.rs`、`AstraEMU/Source/Families/astra-emu-fvp-rfvp-core/src/portable/vm.rs`、`AstraEMU/Source/Families/astra-emu-fvp/src/provider.rs`

**Steps:**

1. 定义 family-private scheduler trace、context id、sequence、budget、wait/yield/fault/terminal 状态和 snapshot cursor。
2. 多线程、多 fiber 或多 context VM 使用 child state machine，并按固定 `(priority, context_id, sequence)` 推进。
3. Basic block 执行到 syscall、branch、wait、fault 或预算耗尽时停止，并输出 action trace。
4. Syscall/action bridge 只输出 `LegacyEffect`、AwaitToken、PresentationCommand、AudioCommand、TextCaptureEvent 和 diagnostic。
5. 编写 scheduler ordering、await boundary、snapshot/replay hash、fault isolation 和 FVP detailed mapping 测试。

**Done Evidence:** `cargo test -p astra-emu-family-api family_scheduler` 和 `cargo test -p astra-emu-fvp state_machine_mapping` 通过；report 输出 `emu.vm_state_machine_trace`、context coverage、await boundary 和 replay hash。

**Linked Test IDs:** `T-S5-EMUCORE-SM-01`

## S5-LEGACY-VFS-01 Legacy pack VFS mounts

**ID:** `S5-LEGACY-VFS-01`

**Status:** `IN_PROGRESS`

**Goal:** 所有 family pack reader 复用 Asset VFS，旧引擎 pack 只作为 `legacy_pack` mount source，不能替代 `.astrapkg`。

**Depends On:** `S2-VFS-01`、`S5-GAME-RUNTIME-01`、[Asset VFS Contract](../../contracts/asset-vfs.md)

**Target Paths:** `AstraEMU/Source/FamilyApi/astra-emu-family-api/src/lib.rs`、`AstraEMU/Source/Families/astra-emu-fvp/src/archive.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/desktop_source.rs`

**Steps:**

1. 为 Artemis PFS、FVP `.bin`、KrKr XP3、BGI PackFile、Siglus Scene.pck、SoftPAL PAC/DAT 和 Minori PAZ 定义 `vfs_provider` capability 和 legacy prefix，例如 `fvp:/...`。
2. Pack reader 输出 entry table hash、`VfsUri`、entry id、offset、size、hash、media kind、compression support 和 diagnostic。
3. Overlay mount 只允许 profile 声明的 key pattern；同 key 多命中没有 allowlist 时 blocking。
4. `.astrapkg` 保存 case profile、reader identity/hash、release report 和 sanitized scenario refs，不保存商业 payload。
5. Release Gate 校验 entry bounds、hash、unsupported compression、reader identity、path/payload redaction 和 package/source consistency。

**Done Evidence:** `cargo test -p astra-emu-family-api legacy_pack_vfs` 和 `cargo test -p astra-release emu_gate` 通过；report 输出 `emu.legacy_pack_vfs`，且不写本地 root、payload、完整脚本或 bytecode。

**Linked Test IDs:** `T-S5-LEGACY-VFS-01`

## S5-MANAGER-01 Manager RuntimeWorld bridge

**ID:** `S5-MANAGER-01`

**Status:** `IN_PROGRESS`

**Goal:** Manager 能启动 `AstraEmuRuntimeProvider`，由 provider 创建 RuntimeWorld、启用 family plugin、打开 `LegacyRuntimeProvider` session、驱动生命周期 StateMachine，并输出 local case report。

**Depends On:** `S5-GAME-RUNTIME-01`、`S5-EMUCORE-SM-01`、`Docs/contracts/astraemu-ipc.md`、`S1-CORE-01`、`S1-PLUGIN-01`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager-core/src/runtime_provider.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/family_host.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/main.rs`、`AstraEMU/Source/Programs/astra-emu-cli/`

**Steps:**

1. 定义 case launch request、profile、runtime provider binding、family selection、`LegacyRuntimeHostCtx` binding 和 report destination。
2. 启动 `AstraEmuRuntimeProvider`，加载项目 package 或 synthetic fixture，启用 selected family plugin。
3. 通过 `AstraEmuRuntimeProvider::open` 建立 RuntimeWorld 和 family session，并让生命周期 StateMachine 在固定 tick 调用 `emu.step`。
4. 建立 input、overlay、diagnostics、TextCaptureEvent 和 presentation/audio command 采集路径。
5. 编写 plugin disabled、permission denied、missing provider、session fault 和 report redaction 测试。
6. 提供显式 family/game directory 的 quick launch，以及只消费物理输入 JSONL、复用 `astra-platform-headless` 的自动化入口；不得旁路 RuntimeWorld 或 family lifecycle。

**Done Evidence:** Manager 不解析 family 私有 VM 内存，不持有 family 文件系统、renderer/audio handle 或 Actor 指针；所有玩家可见输出都来自 `AstraEmuRuntimeProvider` 输出到 RuntimeWorld 的 event/presentation/audio/report。

**Linked Test IDs:** `T-S5-MANAGER-01`、`T-S5-EMU-CLI-01`

## S5-MANAGER-UI-01 Slint Manager 与 runtime overlay

**ID:** `S5-MANAGER-UI-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU Manager、diagnostic/translation/filter overlay 使用 Slint 1.17.1；host 统一持有 winit 0.30 event loop、surface 与 wgpu 29.0.4 device/queue，并复用 shared UI input、semantic、resource 和 render contract。

**Depends On:** `S5-MANAGER-01`、`S2-UI-BACKEND-01`、[ADR 0015](../../adr/0015-ui-backend-provider-split.md)

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager-ui-slint/`、`AstraEMU/Source/Manager/astra-emu-manager/`

**Steps:**

1. 使用 Astra design tokens 和 Slint 组件实现桌面三栏、手机双列/bottom sheet/bottom navigation、移动大屏桌面式布局和游戏 overlay，不让 Slint 类型进入 Manager Core 或 family API。
2. Slint rendering notifier 取得同一套 wgpu 29 `Device`/`Queue`，Astra renderer 直接提供 GPU stage texture；禁止 CPU 整帧回读和跨设备复制。
3. 完成 keyboard/gamepad/touch/IME、focus、screen reader semantics、safe area、overlay input consumption、surface rebuild 和 device loss。
4. Windows 与 Android GPU emulator 形成 E3；Linux、macOS、iOS 关闭 package/provider/host compile E2；Web 对 native family plugin 返回稳定不支持诊断。
5. About 显示 Slint Royalty-free 2.0 规定归因并随包维护第三方 notices。

**Done Evidence:** Windows/Android Manager workflow、同设备 WGPU identity、响应布局、overlay input isolation、report redaction、accessibility 和 provider identity 通过真实程序证据；不能用静态面板、compile-only 或 emulator 结果外推未验证硬件。

**Linked Test IDs:** `T-S5-MANAGER-UI-01`

## S5-FAMILY-01 LegacyRuntimeProvider facade

**ID:** `S5-FAMILY-01`

**Status:** `IN_PROGRESS`

**Goal:** 定义并实现 `LegacyFamilyPluginDescriptor`、`LegacyRuntimeProvider`、`LegacyRuntimeSessionId`、`LegacyRuntimeHostCtx`、`LegacyStepInput`、`LegacyStepOutput`、`LegacyEffect`、`LegacyWaitRequest` 和 `LegacySnapshotEnvelope`。

**Depends On:** `S5-GAME-RUNTIME-01`、`S5-EMUCORE-SM-01`、`S5-LEGACY-VFS-01`、`S5-MANAGER-01`、`Docs/contracts/astraemu-ipc.md`、`Docs/implementation/astraemu-legacy-runtime-framework.md`、`Docs/implementation/provider-plugin-api.md`

**Target Paths:** `AstraEMU/Source/FamilyApi/astra-emu-family-api/src/lib.rs`、`AstraEMU/Source/Manager/astra-emu-manager-core/src/family_loader.rs`

**Steps:**

1. 定义 family descriptor、runtime provider id、format capability、permission、failure classification 和 redaction policy。
2. 定义 lifecycle API：`probe`、`open`、`step`、`save`、`restore`、`shutdown`；`open` 返回 session id，provider 负责区分并行 case。
3. 定义 provider DTO，所有输入输出都是 stable id、hash、section ref、source span、capability diagnostic 和 postcard payload。
4. 让 `step` 返回有序 `LegacyEffect`、`LegacyWaitRequest`、snapshot dirty section、VFS read evidence 和 redaction summary，由 host adapter 应用到 `DeterministicActionContext`。
5. 编写 provider registration、session lifecycle、effect serialization、snapshot envelope、restore compatibility 和 redaction 测试。

**Done Evidence:** family plugin 不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks，family VM state 只存在于 provider session。

**Linked Test IDs:** `T-S5-FAMILY-01`

## S5-AUTOPROBE-01 Manager auto probe

**ID:** `S5-AUTOPROBE-01`

**Status:** `IN_PROGRESS`

**Goal:** Manager 能按固定 family 优先级自动 probe case，并允许用户用 profile 手动覆盖。

**Depends On:** `S5-MANAGER-01`、`S5-FAMILY-01`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager-core/src/probe.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/main.rs`

**Steps:**

1. 定义 `FamilyAutoProbePolicy`，默认顺序为 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori。
2. 让 Manager 逐个调用 family `probe`，收集 marker、confidence、blocker 和 skipped reason。
3. 支持 case profile 显式指定 family/profile，并在 report 中记录 override reason。
4. 无命中或全部 blocker 时进入手动选择，不尝试执行商业脚本。
5. 编写 synthetic multi-family marker、manual override 和 no-match report 测试。

**Done Evidence:** 自动选择结果可复现，report 能解释命中、跳过、覆盖和最终 family。

**Linked Test IDs:** `T-S5-AUTOPROBE-01`

## S5-SCRIPT-01 Trusted Luau patch/decode runtime

**ID:** `S5-SCRIPT-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU 支持用户 Luau 脚本在 Trusted Project Profile 下执行 patch、decode、text/media hook 和 deterministic effect injection。

**Depends On:** `S5-FAMILY-01`、`S3-LUAU-01`、`Docs/contracts/script-vn.md`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager-core/src/patch.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/desktop_source.rs`、Manager launch orchestration

**Steps:**

1. 定义 `TrustedEmuScriptProfile`，统一使用 Luau，不把 Lua/TJS 作为用户脚本语言。
2. 暴露 read-only VFS、patch overlay、decode transform、text/media hook、VM trace、diagnostic 和 effect intent host API。
3. 状态注入只能提交 `LegacyEffect`、Blackboard、input 或 tag intent，并在 fixed tick 边界应用。
4. 禁止 native handle、Actor 指针、raw filesystem、raw network、system call、未授权 key 提取和访问控制规避。
5. 脚本触发禁止能力时隔离禁用该脚本并写入 redacted diagnostic；只有 case profile 明确允许无补丁模式时继续，否则阻断启动。

**Current Evidence:** 每次执行创建 fresh isolated Luau VM；source、memory、instruction、VFS read、intent、overlay count/bytes 均有界，overlay 只在当前 mount memory 中生效并在 unbind 销毁。Manager 只有 profile 显式选择 `trusted` 才读取固定相对 URI `astraemu.patch.luau`；违规或缺文件直接阻断启动，`no_patch` 也必须显式记录。decode transform 会生成 mount-scoped overlay；text/media hook 会在 host 应用前重新校验 replacement 与 VFS URI；deterministic effect 只在 fixed tick 进入 Runtime。正式 release evidence 尚未生成，所以本项仍为 `IN_PROGRESS`。

**Linked Test IDs:** `T-S5-SCRIPT-01`

## S5-TEXT-01 Text dump and translation provider

**ID:** `S5-TEXT-01`

**Status:** `IN_PROGRESS`

**Goal:** `TextCaptureEvent` 进入 Manager 文本管线；首发 translation provider 通过 ECNU Open API 的显式 Responses/SSE 或 Chat Completions profile 更新非权威 overlay，并执行 consent、预算、缓存、secret 与 report redaction policy。

**Depends On:** `S5-MANAGER-01`、`S5-FAMILY-01`、`S4-AI-01`、`S4-AI-04`

**Target Paths:** `AstraEMU/Source/Providers/astra-emu-translation-openai-compatible/`、`AstraEMU/Source/Manager/astra-emu-manager-core/src/library.rs`

**Steps:**

1. Profile 必须显式填写 endpoint、protocol、model、目标语言、上下文 0–32、正文预算和 secret reference；代码不硬编码默认 model，也不在失败后切换 endpoint/protocol/model。
2. 默认最近 10 句，总正文上限 16 KiB；背景、术语表和上下文超限时在句边界确定性截断。
3. 全局一次授权永不自动失效；UI 始终显示 endpoint、model 和发送范围。默认只有 session cache，用户按游戏 opt-in 后才写 SQLite。
4. timeout、限流、transport 和协议错误不阻塞 Runtime；保留原文、记录稳定 diagnostic、有限退避后熔断，只允许用户手动恢复。
5. shipping credential 只存平台 secret store；SQLite、日志、report、save/replay 和 package 只保存 secret reference 或 hash/count/latency/error code。

**Done Evidence:** Responses SSE、显式 Chat adapter、截断、consent、cache、timeout/rate-limit/circuit breaker 与 redaction 测试通过；另有 ignored live test 使用 ignored env，并证明凭据未进入输出。overlay 不改变 runtime replay hash。

**Linked Test IDs:** `T-S5-TEXT-01`

## S5-FILTER-01 AstraEMU FilterGraph presets

**ID:** `S5-FILTER-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU 复用引擎 `FilterGraph`，为旧 VN case 绑定 final-frame 和 per-layer filter preset。

**Depends On:** `S2-MEDIA-04`、`S5-MANAGER-01`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager-core/src/filter.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/stage_renderer.rs`

**Steps:**

1. 定义 `EmuFilterPresetBinding`，包含 final-frame preset 和可选 per-layer role preset。
2. final-frame preset 对 RuntimeWorld 合成后的画面做后处理。
3. per-layer preset 绑定 `PresentationCommand` 的 layer id 或 role；family 缺少 layer metadata 时只启用 final-frame。
4. 输出 missing layer metadata diagnostic，不新增 family 专属 shader/filter API。
5. 编写 final-frame、per-layer、metadata 缺失和 headless hash 测试。

**Done Evidence:** filter preset 使用同一 `FilterGraph` contract；family plugin 不直接持有 renderer handle 或 shader object。

**Linked Test IDs:** `T-S5-FILTER-01`

## S5-ARTEMIS-01 Artemis family plugin

**ID:** `S5-ARTEMIS-01`

**Goal:** Artemis family plugin 支持 PFS/PF6/PF8 probe、boot keys、`.iet` tag、legacy Lua call/filter、presentation/media command、snapshot 和 report。

**Depends On:** `S5-FAMILY-01`、`Docs/emu/artemis/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-artemis/`、`AstraEMU/Tests/artemis/`、`scenarios/emu/artemis_full_flow.yaml` planned target

**Steps:**

1. 实现 PF6/PF8 header、index、entry bounds check、PF8 XOR 和 patch chain resolver。
2. 读取 `system.ini` boot keys，选择 platform section 和 BOOT entry。
3. 解析 `.iet` text/tag、legacy Lua block hash、`.ast` table row 和 ASB classification。
4. 接入 tag filter、enqueueTag、presentation/media command、AwaitToken 和 serializable snapshot allowlist。
5. 编写 synthetic PFS、boot metadata、tag parser、snapshot replay 和 full-flow scenario 测试。

**Done Evidence:** Artemis report 不含商业 payload、私有绝对路径、未授权截图、音频采样或完整脚本。

**Linked Test IDs:** `T-S5-ARTEMIS-01`

## S5-KRKR-01 KrKr family alpha profile

**ID:** `S5-KRKR-01`

**Goal:** KrKr family 输出 alpha probe profile，验证 XP3 probe、virtual storage、script classifier、KAG boot trace、media bridge 和 release report。

**Depends On:** `S5-FAMILY-01`、`Docs/emu/krkr/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-krkr/`、`AstraEMU/Tests/krkr/`、`scenarios/emu/krkr_probe.yaml` planned target

**Steps:**

1. 实现 XP3 index、patch layering 和 virtual storage resolver。
2. 识别 KAG source、TJS bytecode、`.ks.scn`/PSB binary scenario，并为 unsupported branch 输出 diagnostic。
3. 输出 image、voice、BGM、movie command probe 和 boot trace hash。
4. 编写 synthetic fixture、metadata smoke 和 probe scenario 测试。

**Done Evidence:** KrKr alpha report 不含商业 payload、私有绝对路径、未授权截图或音频采样。

**Linked Test IDs:** `T-S5-KRKR-01`

## S5-BGI-01 BGI family plugin

**ID:** `S5-BGI-01`

**Goal:** BGI family plugin 支持 PackFile/BURIKO ARC20、DSC decode、BCS/BP probe、VM memory、host dispatch、media probe 和 report。

**Depends On:** `S5-FAMILY-01`、`Docs/emu/bgi/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-bgi/`、`AstraEMU/Tests/bgi/`、`scenarios/emu/bgi_full_flow.yaml` planned target

**Steps:**

1. 实现 archive index、bounds check、name normalization 和 DSC decode。
2. 实现 BCS、BP、headerless scenario 检测顺序和 parser。
3. 实现 VM memory、stack、PC、program table 和 source map。
4. 实现 Host dispatch diagnostic、AwaitToken、Presentation、Image/Audio/Movie probe。
5. 编写 archive fixture、script fixture、VM dispatch 和 full-flow scenario 测试。

**Done Evidence:** BGI local report 只输出 hash、offset、entry count、opcode histogram 和脱敏 metadata。

**Linked Test IDs:** `T-S5-BGI-01`

## S5-SOFTPAL-01 SoftPAL 接入门槛

**ID:** `S5-SOFTPAL-01`

**Goal:** SoftPAL 在首批 family 稳定后接入，先完成 probe、resource catalog、script VM、extcall diagnostics 和 release gate。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/softpal/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-softpal/`、`AstraEMU/Tests/softpal/`、`scenarios/emu/softpal_full_flow.yaml` planned target

**Steps:**

1. 复用 `LegacyRuntimeProvider` facade，不新增 Manager 私有通道。
2. 实现 PAC/DAT probe、resource catalog 和 script VM alpha route。
3. Unknown extcall 默认输出 diagnostic；presentation/audio/save/control-flow side effect 缺失时 release gate 不算通过。
4. 编写 fixture smoke、extcall report 和 full-flow scenario 测试。

**Done Evidence:** SoftPAL gate 能区分 recoverable diagnostic 和阻断玩家流程的 missing extcall。

**Linked Test IDs:** `T-S5-SOFTPAL-01`

## S5-FVP-01 FVP 接入门槛

**ID:** `S5-FVP-01`

**Status:** `IN_PROGRESS`

**Goal:** FVP 作为 v1 首发 family，以固定 rfvp revision 为行为基线，覆盖 probe、archive/media resolver、完整 HCB VM/syscall、presentation/audio/movie/input 与 save/load/snapshot/replay。

**Depends On:** `S5-FAMILY-01`、`S5-GAME-RUNTIME-01`、`S5-LEGACY-VFS-01`、`Docs/emu/fvp/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-fvp/`、`AstraEMU/Source/Families/astra-emu-fvp-rfvp-core/`、`Tools/verify_fvp_parity.py`

**Steps:**

1. 固定并记录 rfvp revision、MPL-2.0 notice、修改记录与 source offer；合法输入逐字节对齐 parser、0x00..0x27 opcode、Variant、stack/call frame、context/thread request、read-state 和 syscall 可观察行为。
2. 实现 `.bin` VFS、HZC1/NVSG、Ogg/RIFF、WMV/MP4 compatibility probe、cursor、graph/prim/text/audio/movie/input/save/load；路径逃逸、损坏输入、越界和预算失控确定性 fail-fast。
3. 把 HCB basic block 映射为 family-private action sequence，把有序 effect/wait/trace/coverage/snapshot hint 交给 `AstraEmuRuntimeProvider`。
4. 覆盖 148 个 release syscall；任何未实现分支、软失败临时代码或 unknown dispatch 都让 coverage gate blocking，不能返回 `Nil` 隐藏缺失行为。
5. 提交 synthetic fixture 与 sanitized golden；商业样本只生成 ignored local parity report，不进入仓库。

**Current Evidence:** 148 个 release syscall 均有显式 handler，并通过 catalog identity 与 panic-free neutral probe；synthetic archive/HCB proptest、session lifecycle、self-contained texture snapshot、host-command audio restore ordering、text/audio effect 和 product-provider replay 测试已通过。Windows 本机授权样本的 ignored Headless run 已完成 240 tick、240 frame 和 snapshot round-trip，report 不含路径或 payload；checkpoint 未证明视觉变化，证据等级仍为 E2。`Tools/verify_fvp_parity.py` 会从官方仓库取固定 commit，在临时 detached worktree 中运行同一套脱敏 parser/opcode/Variant/context/call/syscall trace，再与仓库 golden 逐字段比较。该 golden 只覆盖 VM/parser 合法输入，不代表 graph/text/audio/movie/save/load 全流 parity；Windows/Android E3 也未完成，因此不能标记 `DONE`。

**Linked Test IDs:** `T-S5-FVP-01`

## S5-SIGLUS-01 Siglus 接入门槛

**ID:** `S5-SIGLUS-01`

**Goal:** Siglus 在首批 family 稳定后接入，覆盖 root probe、Scene.pck、Gameexe、`.ss` script、G00/media 和 report policy。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/siglus/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-siglus/`、`AstraEMU/Tests/siglus/`、`scenarios/emu/siglus_full_flow.yaml` planned target

**Steps:**

1. 复用 `LegacyRuntimeProvider` facade 和 failure classification。
2. 实现 Siglus root、Scene.pck、Gameexe header 和授权 material 缺失 diagnostic。
3. 实现 `.ss` header、string table、label、operand decoder 和 basic stack model。
4. 实现 G00/Ogg/OVK/NWA/OMV probe，受保护 stream 只消费用户合法提供的材料。
5. 编写 probe-only report、script fixture 和 full-flow scenario 测试。

**Done Evidence:** Siglus report 不包含 key、payload transform、未授权截图或私有 stream。

**Linked Test IDs:** `T-S5-SIGLUS-01`

## S5-GATE-01 AstraEMU release gate

**ID:** `S5-GATE-01`

**Status:** `IN_PROGRESS`

**Goal:** Release Gate 检查 FVP full-flow、`LegacyRuntimeProvider` facade、显式 runtime/family/UI binding、Slint/WGPU/toolchain/license identity、Trusted Luau、ECNU translation policy、filter、snapshot/replay、host identity 与 report redaction。

**Depends On:** `S5-FAMILY-01`、`S5-AUTOPROBE-01`、`S5-SCRIPT-01`、`S5-TEXT-01`、`S5-FILTER-01`、`S5-FVP-01`、`S5-MANAGER-UI-01`

**Target Paths:** `Engine/Source/Developer/astra-release/src/emu.rs`、`AstraEMU/Source/Manager/astra-emu-manager-core/src/evidence.rs`、`AstraEMU/Source/Programs/astra-emu-evidence/`

**Steps:**

1. 增加 explicit runtime/family/UI binding、Slint/wgpu/toolchain/license identity、FVP full-flow/syscall/parity/snapshot/replay、Trusted Luau 与 translation consent/provider/cache checks。
2. 校验 plugin ABI/engine/rustc/feature fingerprint、binary hash、package eligibility、官方签名、Android APK/native manifest 或 iOS static registration binding。
3. 校验 Windows/Android run identity 绑定同一 build/profile/package/session/input sequence，以及视觉、音频、输入消费、route/terminal 和 surface lifecycle evidence。
4. 所有 report 只允许 alias/hash/offset/size/count/diagnostic；绝对路径、URI、商业 payload、secret、未授权截图/音频或访问控制规避材料必须 blocking。
5. 编写 missing/conflicting provider、missing syscall、signature mismatch、denied script、translation consent/cache 和 payload redaction 失败测试。

**Current Evidence:** release gate 已有 14 项 fail-closed check，并以完整 passing fixture 验证 provider/UI/FVP/Luau/translation/六平台 continuity。`astra-emu-evidence` 会在写入 package sections 前拒绝 unknown field、payload-like field、绝对路径、identity drift 和不完整 E2/E3 lifecycle。真实平台 evidence 尚未生成，不能把 passing fixture 当作发布证据。

**Linked Test IDs:** `T-S5-GATE-01`

## S5-PROGRAM-TARGET-01 AstraEMU Manager 与 CLI Program target

**ID:** `S5-PROGRAM-TARGET-01`

**Goal:** AstraEMU Manager 与 `astra-emu-cli` 以 `Program` target 运行；被启动的 case 通过 `AstraEmuRuntimeProvider` 作为 `Game` runtime session 运行，family plugin 仍通过 `LegacyRuntimeProvider` 注册，不升级成独立 Game target。CLI native path 只负责显式 quick launch，Headless path 复用 `astra-platform-headless` 与物理输入协议。

**Depends On:** `S1-TARGET-01`、`S5-MANAGER-01`、`S5-FAMILY-01`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager/src/main.rs`、`AstraEMU/Source/Manager/astra-emu-manager/Cargo.toml`、`AstraEMU/Source/Programs/astra-emu-cli/`、`AstraEMU/Platforms/`

**Steps:**

1. 定义 `astra-emu-manager` Target，kind 为 `program`，绑定 desktop platforms。
2. Manager 启动时校验 Program target 和 platform capability。
3. `AstraEmuRuntimeProvider` 的 case target 与 Manager Program target 分开校验。
4. family plugin descriptor 只进入 plugin registry，不写成独立 Target。
5. 编写 Manager target validation、case runtime provider handoff、family plugin isolation 和 local case report 测试。

**Done Evidence:** Manager report 包含 Program target id，family report 仍只记录 provider id 和 session id。

**Linked Test IDs:** `T-S5-PROGRAM-TARGET-01`

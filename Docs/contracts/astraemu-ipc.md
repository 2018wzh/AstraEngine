# AstraEMU Legacy Runtime Provider Contract

AstraEMU v1 采用 Manager + `AstraEmuRuntimeProvider` + AstraEngine `RuntimeWorld` + in-process family plugin 架构。Manager 负责窗口、输入、配置、project policy、插件启用、报告和 overlay；`AstraEmuRuntimeProvider` 是 gameplay runtime provider；`RuntimeWorld` 持有 tick、MutationLog、Save/Replay 和 Release Gate 语义；family plugin 通过 `LegacyRuntimeProvider` facade 把旧引擎行为转成可审计的 Runtime effect 和 save section。

`EMUCoreBridge` 只作为 extension point 保留，用于外部工具或研究环境。它不属于 v1 主路径，也不能替换 `RuntimeWorld`。

## v13 迁移状态

当前 hard-cut identity 为 `astra.emu.family_abi.v13`。v7/v8/v9/v10/v11/v12 module、fingerprint 与旧 runtime snapshot 必须在 provider 执行前拒绝；没有 compatibility shim。Product Runtime Provider ABI 保持 v4，Extension ABI identity 为 `astra.emu.extension_abi.v1`。v13 保留有界 typed system-menu 与 confirmation transaction，并新增同一 Family ABI session 的 typed system-command publication port。

Family 只发布菜单层级、启用状态、勾选状态和确认语义；Host 负责按当前平台能力呈现，并把 `Select`、`Dismiss`、`Accepted` 或 `Cancelled` 送回同一 session。Family action 只在已验证的结果进入固定 step 后执行，Host/Manager/CLI 不按 item 名称或确认文本解释 family 语义。Manager 的宿主界面使用 Slint，桌面 CLI 通过 `astra-platform` 的原生 context-menu/confirmation provider；Headless 使用序列化物理输入，不伪造桌面 native evidence。

Windows Host 使用 Win32 `muda-win` 和原生 message dialog；macOS Host 使用 AppKit `muda`，在主线程按 live winit view 进行菜单追踪并转换 flipped-view 锚点。Family 选择的窗口/帮助命令也只通过 typed system-command 交给 Host：Windows/macOS 对已绑定窗口执行原生全屏、原始尺寸恢复、缩放采样切换、HTML Help、About 对话框和系统浏览器打开；未绑定能力必须返回 `Applied`、`Rejected` 或 `Unsupported`，不能由 Family 或 Manager 自行改写窗口。缩放抗锯齿由 `astra-platform-common` 的 presentation core 在 GPU sampler 间切换，不重建 Family scene。当前 Linux Wayland Host 没有 GTK window 绑定，原生 context-menu 请求必须返回 `ASTRA_EMU_PLATFORM_CONTEXT_MENU_UNSUPPORTED`，不能保留 pending transaction 或隐式切换到 Manager/Headless UI。该差异是平台能力边界，不改变 Family ABI 的语义 ownership。

本次 ABI 契约已经落地，FVP、Minori、Manager、CLI、Headless 与平台 renderer 的 consumer 迁移仍是 `IN_PROGRESS`。v7 的 scene transaction、snapshot/save/restore、text lease、session resource presentation 与 step budget 只属于历史实现，不是当前接口能力。

## Descriptor

```rust
pub struct LegacyFamilyPluginDescriptor {
    pub family_id: FamilyId,
    pub plugin_id: PluginId,
    pub engine_version: SemVer,
    pub feature_fingerprint: String,
    pub supported_formats: Vec<LegacyFormatId>,
    pub runtime_provider: ProviderId,
    pub permissions: Vec<PermissionId>,
    pub report_redaction: RedactionPolicyId,
    pub core_kind: FamilyCoreKind,
    pub presentation_mode: FamilyPresentationMode,
}
```

descriptor 必须通过 plugin fingerprint、capability、permission、license 和 family feature gate。只接受 `Native + MultiLayer` 和 `Ported + SingleLayer`；FVP 固定使用 `Ported + SingleLayer`，Minori 固定使用 `Native + MultiLayer`。错误组合必须在 session 创建前阻断。

Product runtime descriptor 必须声明唯一 `PresentationLane::{Scene2D, Layer2D}`。AstraVN 使用 `Scene2D`，AstraEMU 使用 `Layer2D`，单个 session 禁止混用。

## Runtime Provider

`LegacyRuntimeProvider` 是 family runtime 的唯一 public facade。provider 位于 `AstraEmuRuntimeProvider` 之下，可以在内部拆分 archive reader、script VM、renderer、native save 和 diagnostics，但这些模块不成为顶层 AstraEngine gameplay provider。

```rust
pub trait LegacyRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor;

    fn probe(
        &self,
        ctx: LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> ProviderResult<LegacyProbeReport>;

    fn open(
        &self,
        ctx: LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> ProviderResult<LegacyRuntimeSessionId>;

    fn step(
        &self,
        ctx: LegacyRuntimeHostCtx,
        session: LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> ProviderResult<LegacyStepOutput>;

    fn shutdown(
        &self,
        ctx: LegacyRuntimeHostCtx,
        session: LegacyRuntimeSessionId,
    ) -> ProviderResult<LegacyShutdownReport>;
}
```

Family ABI v13 对 descriptor、instance、probe、open、step、surface、Hook、writable-file、system-menu、confirmation、system-command、只读 VFS 与 shutdown 使用显式 `StableAbi` wire DTO。字符串、数组、optional/result 和 map 分别使用 `RString`、`RVec`、`ROption`/`RResult` 与有序 pair list；serde 类型仍是业务契约真源，wire 层只做明确转换。

`LegacySystemMenuTransactionV1` 最多包含 64 项，菜单深度最多为 4。重复 id、重复 sibling order、无效 parent、空 submenu、不可选 item 或错配 menu id 都会阻断。Windows native host 通过平台 context-menu provider 显示菜单；Manager 使用同一 transaction 构建 Slint overlay；Headless 只接受序列化物理方向键、确认键和取消键，不提供语义化选项快捷命令。菜单选中的窗口、帮助和关于操作会生成有界 `LegacySystemCommandTransactionV1`；Host 只按 typed kind 选择原生平台能力，结果在下一固定 step 以 `LegacySystemCommandResultV1` 回传。Family 不接收窗口句柄、URL 或本地路径，也不依赖通用事件字符串。

只读 VFS range 请求绑定 expected revision、offset、length 与 bounds，Host 不传文件句柄或本地路径，也不计算 per-read content hash。Family 只能 acquire Host-owned writable surface。lease 明确携带 RGBA8/BGRA8 sRGB premultiplied-alpha format、dimensions、stride 与 generation；core 写入后提交 `Unchanged`、`Full` 或 surface 像素坐标 `Rects` damage。step 成功且 Layer transaction 验证完成后才公开 staged generation；失败时整批回收。pool 暂时耗尽时重试同一 generation 并输出节流 WARN，不临时分配、不丢帧、不切换 presentation mode。设备丢失、整数溢出、尺寸或 stride 不匹配、所有权错误与实际分配失败继续 fail-fast。

Typed PCM 由 `LegacyLiveOutput.audio` 直接携带 `LegacyAudioPacketV7` 和 ABI-owned
`I16`/`F32` buffer，不再拆成 control、bulk reference 和第二条 payload envelope。
Family FFI、Manager 与音频队列按所有权移动同一 allocation，并校验 stream id、采样格式、
sample count、channel count 和边界。相同格式不得重建 PCM；只有 decoder 或 resampler 的
必要格式转换可以生成新 allocation，并单独计入 conversion bytes。格式、长度或边界不匹配
均返回 blocking diagnostic。

`open` 返回 `LegacyRuntimeSessionId`。session 持有 family 私有 VM state、resource resolver、presentation/audio state、await state 与 trace cursor。Manager 可以并行 probe 多个 case，也可以在测试里同时打开多个 session；provider 必须用 session id 隔离状态。

## Retained Layer2D

公共 Layer2D contract 包含 `Layer2DId`、`Surface2DId`、`Layer2DRole`、`Layer2DTransaction`、retained `Create/Update/Destroy`、`Layer2DDamage` 与 `Layer2DContent`。layer state 明确携带 `z_index`、role、transform、clip、opacity、nearest/linear、opaque/alpha/add/multiply/screen 和 typed `FilterGraph`。

Host 按 `z_index` 排序，相同 z 按稳定 `Layer2DId` 排序。越界 damage、过期 generation、重复 create、未知 update/destroy 或无效 transform 会阻断整笔 transaction。公共 Provider ABI 允许 `WritableSurface` 与 `TextureResource`，Family ABI 只允许 writable surface。Host 把 retained state lowering 到现有 `SceneCommand`、`Mesh2D`、texture update 和 FilterGraph executor，不创建第二套 GPU backend。

## Hook 与翻译 companion

Extension ABI v1 的通用 Hook 只识别 family/game/hook/invocation identity 与 opaque owned bytes，Host 不解释 payload。translation companion 只定义 UTF-8 request/response。Manager、CLI 与 Headless 按 `(family_id, family_game_id)` 持有显式启用状态、唯一 provider id 与 `u32 timeout_ms`；默认 2000 ms，0 表示立即超时。

Hook 必须发生在 framebuffer acquire 之前。未绑定时交给 core 处理；FVP 与 Minori 使用原文。成功结果由 family core 完成字体 fallback、shaping、换行和绘制。timeout、认证、限流、网络、协议、缺字或布局失败时保留原文并返回 typed diagnostic。没有异步 completion、晚到结果、翻译 cache、文本 hash或 Host overlay；正文、secret 和 payload 不进入日志、SQLite、report 或 package。

## 原生存档文件

Family ABI 不提供 save/restore/snapshot。每个 game 获得独立 writable root，只能通过安全相对路径调用 stat/list/create-dir/read-range/write-range/set-length/remove/atomic-replace；本地路径和文件句柄不跨 ABI。同一 `(family_id, family_game_id)` 只允许一个 writable session。`atomic-replace` 必须同步临时文件、原子替换并同步父目录。Host/Manager 不定义 save slot，文件组织与格式归游戏/core 所有；AstraEMU Runtime Provider 对共享 save/restore lifecycle 返回 unsupported。

Family-owned system UI 通过 `astra.emu.system_ui_active` blackboard observation 转移物理输入所有权。值只能是规范的 `true` 或 `false`，同一输出批次不得重复。值为 `true` 时，Host 必须保留底层 gameplay wait，只把物理输入交给 family；页面关闭只恢复输入所有权，不能伪造 await completion。页面 identity 仍归 family 私有 observation，CLI 和 Manager 不按 family 页面名称推断输入所有权。

## Host Context

```rust
pub struct LegacyRuntimeHostCtx {
    pub case_id: StableId,
    pub package: PackageRef,
    pub read_mount: VfsMountSetRef,
    pub media_services: MediaServiceRefs,
    pub report_sink: ReportSinkRef,
    pub permission_policy: PermissionPolicyRef,
}
```

Host context 只传 ABI-safe value、stable id、hash、section ref、VFS mount set ref、source span、capability ref 和 DTO。旧 VM 指针、Actor 指针、`RuntimeWorld` 指针、platform file descriptor、renderer/audio native handle、Editor widget、商业 payload 和完整脚本文本不得进入 public API。

## Manager Modernization DTO

这些 DTO 属于 AstraEMU Manager 和 plugin/provider contract，不进入 family VM public API。它们只选择、包裹或消费 `LegacyRuntimeProvider` 的输出。

```rust
pub struct FamilyAutoProbePolicy {
    pub priority: Vec<FamilyId>,
    pub manual_override: Option<LegacyFamilyProfileId>,
    pub selected: Option<FamilyId>,
    pub diagnostics: Vec<LegacyProbeDiagnostic>,
}

pub struct TextCapturePipeline {
    pub local_dump: TextDumpPolicy,
    pub translation_provider: Option<ProviderId>,
    pub overlay_policy: TranslationOverlayPolicy,
    pub redaction: RedactionPolicyId,
}

pub struct EmuFilterPresetBinding {
    pub final_frame: Option<FilterGraphRef>,
    pub per_layer_roles: Vec<LayerFilterBinding>,
}
```

默认 auto probe 顺序是 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori。用户 profile 可以覆盖最终 family。AstraEMU 不执行通用 Luau patch 或 decoder callback；family 的私有配置由严格 launch profile 与安全相对 private file 交给对应 factory。旧 Lua/TJS 只描述 family 内部 legacy 事实，不构成 Host 脚本接口。缺少配置、schema 不匹配或 private file 越界时直接阻断启动，不允许无补丁回退。

Text dump 默认只写 hash、长度、source ref 和 speaker metadata；用户本地 opt-in 后才能保存全文 dump。翻译 overlay 是非权威 UI 状态，不进入 replay hash。Filter preset 复用 `FilterGraph`；family 缺少 layer metadata 时，只启用 final-frame preset 并输出 diagnostic。

## Step Contract

```rust
pub struct LegacyStepInput {
    pub tick_index: u64,
    pub frame_time_ms: u32,
    pub input_edges: Vec<LegacyInputEdge>,
    pub await_results: Vec<LegacyAwaitResult>,
    pub provider_results: Vec<LegacyProviderResult>,
    pub replay_mode: ReplayMode,
}

pub struct LegacyStepOutput {
    pub status: LegacyRuntimeStatus,
    pub live: LegacyLiveOutput,
    pub control: LegacyControlTransaction,
    pub trace: Vec<StateMachineTrace>,
    pub diagnostics: Vec<Diagnostic>,
    pub coverage: LegacyCoverageDelta,
}
```

Runtime 每个 tick 按固定顺序把 input、await result 和 provider result 交给 provider。provider 在 family session 内推进旧 VM，直到遇到 wait、halt、fault 或 presentation boundary。所有输出必须在本 tick 结束前变成有序 `LegacyStepOutput`。Family step 不携带策略预算；ABI 表示、checked arithmetic、buffer/stride、所有权、路径隔离和系统错误仍是阻断条件。Performance E2 是唯一预算型阻断门禁。

Family session 可以把旧 VM 映射为私有 scheduler、context、basic-block 和 action 状态机。多线程、多 fiber 或多 context VM 必须由 deterministic scheduler 推进，排序键固定为 `(priority, context_id, sequence)`。Host 只接收 `LegacyStepOutput` 与 diagnostic，不读取 family private child state。

## Typed Live And Control

```rust
pub struct LegacyLiveOutput {
    pub layers: Vec<Layer2DTransaction>,
    pub audio: Vec<LegacyAudioPacketV7>,
    pub audio_commands: Vec<LegacyAudioCommandV7>,
    pub video: Vec<LegacyVideoCommandV7>,
}

pub enum LegacyWaitRequest {
    Frame { frames: u32 },
    Time { milliseconds: u32 },
    Input { mask: LegacyInputMask },
    MediaFence { media_id: StableId },
    PresentationFence { fence_id: StableId },
    ProviderCompletion { request_id: StableId },
}
```

Framework adapter 只把轻量 `LegacyControlTransaction` 原子提交到 `DeterministicActionContext`；Layer2D、PCM 和其他 live allocation 在 transaction 成功后直接移动给 Host owner。任何异步 IO、decode、timer、audio/video completion 和平台回调都必须变成 typed completion，在下一 fixed tick 回到 `step`。

## Hash 与可观测性

AstraEMU state/snapshot/text/frame/audio/route/session/input 与 RFVP live 路径不生成运行时语义 hash。package、plugin binary、source/archive entry、schema、build、profile 与 artifact-file 完整性 hash 继续保留。事件只记录稳定 diagnostic code、identity、状态和计数；正文、secret、payload、本地路径与文本 hash 不得写入日志或 evidence。

## Runtime Flow

```text
AstraEMU Manager
  -> select AstraEmuRuntimeProvider
  -> create RuntimeWorld
  -> enable family plugin
  -> open LegacyRuntimeProvider session
  -> register gameplay StateMachine action adapter
  -> tick RuntimeWorld
  -> StateMachine invokes emu.step
  -> atomically apply LegacyControlTransaction
  -> validate and publish retained Layer2D transaction
  -> move typed PCM / video output to host owners
  -> write LocalCaseReport
```

family plugin 可以持有 private interpreter state，但权威推进必须通过 StateMachine typed action 和 control transaction。实时输出不编码、不计算 content hash。

## VFS And Pack Readers

旧引擎资源包通过 Asset VFS 挂载为 `legacy_pack`。Family reader 只能实现注册到 `vfs_provider` slot 的 VFS provider，不能替代 `.astrapkg` 或直接读取 host filesystem。`.astrapkg` 保存 case profile、family provider binding、reader identity/hash、sanitized scenario refs 和 release report；legacy pack mount 提供 provider URI、entry map、offset、size、hash、media kind 和 bounded read。

Patch、翻译覆盖和本地调试替换走 `overlay` mount。未声明 overlay allowlist 的同 `VfsUri` 多命中必须 blocking。Report 只记录 `vfs_uri`、prefix、pack/entry、offset、size、hash、media kind、coverage 和 diagnostic，不记录本地 root、payload、完整脚本或 bytecode。

## Family 顺序

v1 首发 family 是 FVP。Artemis 与 KrKr/KAG/TJS、BGI/Ethornell、SoftPAL、Siglus、Minori 作为后续 family。所有 family 复用同一 `LegacyRuntimeProvider` contract、VFS mount contract 和 release gate；私有格式知识留在 family session 内，不反向扩展 EngineCore 对象模型。

## Report

Local case report 只包含 hash、coverage、diagnostics、命令、family feature、redaction status 和脱敏 metadata，不包含商业 payload、私有绝对路径、未授权截图、音频采样、provider secret 或可绕过访问控制的说明。

```bash
astra emu probe cases/artemis-synthetic --family artemis --report target/reports/emu-probe.yaml
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
```

Expected report includes `emu.legacy_runtime_provider`, `emu.artemis_full_flow`, `emu.report_redaction` and `plugin.extension_registry`.

## Native replay

`astra-emu-cli run --input` 在正式 native host 中消费同一份 `astra.user_input_sequence.v1` JSONL。输入序列会在 host 启动前完整校验；replay 存在时，窗口键盘、指针、触摸、手柄和 IME gameplay input 会在 host 边界被拒绝。窗口生命周期事件仍按平台契约处理。

`run` 不生成 E2 report。自动截图、WAV、checkpoint 和 machine-readable run report 只归 Headless E2；Release CLI 的 Sandbox 结果只能记录为行为/视觉验收，不能替代正式 Windows E3。

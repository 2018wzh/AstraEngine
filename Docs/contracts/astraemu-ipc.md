# AstraEMU Legacy Runtime Provider Contract

AstraEMU v1 采用 Manager + `AstraEmuRuntimeProvider` + AstraEngine `RuntimeWorld` + in-process family plugin 架构。Manager 负责窗口、输入、配置、project policy、插件启用、报告和 overlay；`AstraEmuRuntimeProvider` 是 gameplay runtime provider；`RuntimeWorld` 持有 tick、MutationLog、Save/Replay 和 Release Gate 语义；family plugin 通过 `LegacyRuntimeProvider` facade 把旧引擎行为转成可审计的 Runtime effect 和 save section。

`EMUCoreBridge` 只作为 extension point 保留，用于外部工具或研究环境。它不属于 v1 主路径，也不能替换 `RuntimeWorld`。

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
}
```

descriptor 必须通过 plugin fingerprint、capability、permission、license 和 family feature gate。family plugin 不能声明替换 Runtime tick、Save container、MutationLog、Release Gate core checks 或 renderer/audio native handle。

## Runtime Provider

`LegacyRuntimeProvider` 是 family runtime 的唯一 public facade。provider 位于 `AstraEmuRuntimeProvider` 之下，可以在内部拆分 archive reader、script VM、media bridge、snapshot serializer 和 diagnostics，但这些模块不成为顶层 AstraEngine gameplay provider。

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

    fn save(
        &self,
        ctx: LegacyRuntimeHostCtx,
        session: LegacyRuntimeSessionId,
        request: LegacySnapshotSaveRequest,
    ) -> ProviderResult<LegacySnapshotEnvelope>;

    fn restore(
        &self,
        ctx: LegacyRuntimeHostCtx,
        session: LegacyRuntimeSessionId,
        snapshot: LegacySnapshotEnvelopeRef,
    ) -> ProviderResult<LegacyRestoreReport>;

    fn shutdown(
        &self,
        ctx: LegacyRuntimeHostCtx,
        session: LegacyRuntimeSessionId,
    ) -> ProviderResult<LegacyShutdownReport>;
}
```

`open` 返回 `LegacyRuntimeSessionId`。session 持有 family 私有 VM state、resource resolver、legacy presentation/audio state、await state、snapshot cursor 和 trace cursor。Manager 可以并行 probe 多个 case，也可以在测试里同时打开多个 session；provider 必须用 session id 隔离状态。

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

pub struct TrustedEmuScriptProfile {
    pub script_bundle: PackageSectionRef,
    pub trusted_profile: bool,
    pub host_capabilities: Vec<PermissionId>,
    pub violation_policy: ScriptViolationPolicy,
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

默认 auto probe 顺序是 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori。用户 profile 可以覆盖最终 family。Luau 是唯一用户脚本语言；旧 Lua/TJS 只描述 family 内部 legacy 事实。Trusted script 可以提交 `LegacyEffect`、Blackboard、input 或 tag intent，但这些 intent 必须在 fixed tick 边界进入 Runtime。脚本请求未授权 key 提取、商业保护处理或访问控制规避时，Manager 隔离禁用该脚本，并继续以无补丁模式运行 case。

Text dump 默认只写 hash、长度、source ref 和 speaker metadata；用户本地 opt-in 后才能保存全文 dump。翻译 overlay 是非权威 UI 状态，不进入 replay hash。Filter preset 复用 `FilterGraph`；family 缺少 layer metadata 时，只启用 final-frame preset 并输出 diagnostic。

## Step Contract

```rust
pub struct LegacyStepInput {
    pub tick_index: u64,
    pub frame_time_ms: u32,
    pub input_edges: Vec<LegacyInputEdge>,
    pub await_results: Vec<LegacyAwaitResult>,
    pub provider_results: Vec<LegacyProviderResult>,
    pub budget: LegacyStepBudget,
    pub replay_mode: ReplayMode,
}

pub struct LegacyStepOutput {
    pub status: LegacyRuntimeStatus,
    pub effects: Vec<LegacyEffect>,
    pub waits: Vec<LegacyWaitRequest>,
    pub trace: Vec<StateMachineTrace>,
    pub diagnostics: Vec<Diagnostic>,
    pub snapshot_hint: Option<LegacySnapshotHint>,
    pub coverage: LegacyCoverageDelta,
}
```

Runtime 每个 tick 按固定顺序把 input、await result 和 provider result 交给 provider。provider 在 family session 内推进旧 VM，直到遇到 wait、halt、fault、预算耗尽或 presentation boundary。所有输出必须在本 tick 结束前变成有序 `LegacyStepOutput`。

Family session 可以把旧 VM 映射为私有 scheduler、context、basic-block 和 action 状态机。多线程、多 fiber 或多 context VM 必须由 deterministic scheduler 推进，排序键固定为 `(priority, context_id, sequence)`。Host 只接收 `LegacyStepOutput`、trace 和 snapshot envelope，不读取 family private child state。

## Effects And Wait

```rust
pub enum LegacyEffect {
    RuntimeEvent(RuntimeEvent),
    Presentation(PresentationCommand),
    Audio(AudioCommand),
    TextCapture(TextCaptureEvent),
    Trace(StateMachineTrace),
    SetBlackboard { key: String, value: BlackboardValue },
    Await(AwaitToken),
    ScheduleEvent(ScheduledRuntimeEvent),
    SnapshotSection(PackageSectionRef),
}

pub enum LegacyWaitRequest {
    Frame { frames: u32 },
    Time { milliseconds: u32 },
    Input { mask: LegacyInputMask },
    MediaFence { media_id: StableId },
    PresentationFence { fence_id: StableId },
    ProviderCompletion { request_id: StableId },
    FamilyOpaque { kind: String, payload_hash: Hash256 },
}
```

Framework adapter 把 `LegacyEffect` 逐条应用到 `DeterministicActionContext`。任何异步 IO、decode、timer、audio/video completion 和平台回调都必须变成 `AwaitToken` 或 provider result，在下一 fixed tick 回到 `step`。Replay 消费录制结果，不重新询问平台 provider。

## Snapshot

```rust
pub struct LegacySnapshotEnvelope {
    pub family_id: FamilyId,
    pub session_id: LegacyRuntimeSessionId,
    pub schema_version: SchemaVersion,
    pub case_fingerprint: Hash256,
    pub runtime_cursor: LegacyRuntimeCursor,
    pub family_sections: Vec<PackageSection>,
    pub redaction: RedactionStatus,
}
```

Snapshot envelope 是公共壳，family section 是 opaque postcard payload。Manager 和 EngineCore 只能校验 section id、version、hash、migration manifest 和 redaction，不解析 family VM stack、opcode state、TJS object、Lua state、Siglus scene stream 或旧引擎 presentation object。

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
  -> adapter applies LegacyEffect list
  -> collect RuntimeEvent / PresentationCommand / AudioCommand / TextCaptureEvent
  -> write LocalCaseReport
```

family plugin 可以持有 private interpreter state，但权威推进必须通过 StateMachine action 和可序列化 effect list。每个 effect 在固定 tick 边界进入 Runtime，有 source span、trace id 和 replay hash。

## VFS And Pack Readers

旧引擎资源包通过 Asset VFS 挂载为 `legacy_pack`。Family reader 只能实现注册到 `vfs_provider` slot 的 VFS provider，不能替代 `.astrapkg` 或直接读取 host filesystem。`.astrapkg` 保存 case profile、family provider binding、reader identity/hash、sanitized scenario refs 和 release report；legacy pack mount 提供 provider URI、entry map、offset、size、hash、media kind 和 bounded read。

Patch、翻译覆盖和本地调试替换走 `overlay` mount。未声明 overlay allowlist 的同 `VfsUri` 多命中必须 blocking。Report 只记录 `vfs_uri`、prefix、pack/entry、offset、size、hash、media kind、coverage 和 diagnostic，不记录本地 root、payload、完整脚本或 bytecode。

## Family 顺序

v1 可用 family 是 Artemis。后续按通用性排序扩展：KrKr/KAG/TJS、BGI/Ethornell、SoftPAL、FVP、Siglus、Minori。所有 family 复用同一 `LegacyRuntimeProvider` contract、VFS mount contract 和 release gate；私有格式知识留在 family session 内，不反向扩展 EngineCore 对象模型。

## Report

Local case report 只包含 hash、coverage、diagnostics、命令、family feature、redaction status 和脱敏 metadata，不包含商业 payload、私有绝对路径、未授权截图、音频采样、provider secret 或可绕过访问控制的说明。

```bash
astra emu probe cases/artemis-synthetic --family artemis --report target/reports/emu-probe.yaml
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
```

Expected report includes `emu.legacy_runtime_provider`, `emu.artemis_full_flow`, `emu.report_redaction`, `runtime.replay.determinism` and `plugin.extension_registry`.

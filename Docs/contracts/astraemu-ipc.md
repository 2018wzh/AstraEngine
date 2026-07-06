# AstraEMU Family Plugin Contract

AstraEMU v1 采用 Manager + AstraEngine `RuntimeWorld` + in-process family plugin 架构。Manager 负责窗口、输入、配置、project policy、插件启用、报告和 overlay；RuntimeWorld 持有 tick、MutationLog、Save/Replay 和 Release Gate 语义；family plugin 只把旧引擎语义翻译成引擎可审计的 provider、action 和 section。

`EMUCoreBridge` 保留为普通 extension point，用于外部工具桥接或研究环境；它不属于 v1 主路径，也不能替换 `RuntimeWorld`。

## Descriptor

```rust
pub struct LegacyFamilyPluginDescriptor {
    pub family_id: FamilyId,
    pub plugin_id: PluginId,
    pub engine_version: SemVer,
    pub feature_fingerprint: String,
    pub supported_formats: Vec<LegacyFormatId>,
    pub providers: Vec<LegacyProviderKind>,
    pub permissions: Vec<PermissionId>,
    pub report_redaction: RedactionPolicyId,
}
```

descriptor 必须通过 plugin fingerprint、capability、permission、license 和 family feature gate。family plugin 不能声明替换 Runtime tick、Save container、MutationLog、Release Gate core checks 或 renderer/audio native handle。

## Provider API

```rust
pub trait LegacyVfsProvider {
    fn probe(&self, request: LegacyProbeRequest) -> ProviderResult<LegacyProbeReport>;
    fn mount(&self, request: LegacyMountRequest) -> ProviderResult<LegacyVfsMount>;
    fn read_entry(&self, entry: LegacyEntryRef) -> ProviderResult<LegacyBytesRef>;
}

pub trait LegacyScriptProvider {
    fn classify(&self, entry: LegacyEntryRef) -> ProviderResult<LegacyScriptKind>;
    fn compile_actions(&self, request: LegacyCompileRequest) -> ProviderResult<LegacyActionGraph>;
}

pub trait LegacyActionProvider {
    fn invoke(&self, request: LegacyActionRequest) -> ProviderResult<LegacyActionEffects>;
}

pub trait LegacyMediaMapper {
    fn map_media(&self, request: LegacyMediaRequest) -> ProviderResult<PresentationCommand>;
    fn map_audio(&self, request: LegacyAudioRequest) -> ProviderResult<AudioCommand>;
}

pub trait LegacySnapshotCodec {
    fn save(&self, request: LegacySnapshotSaveRequest) -> ProviderResult<PackageSection>;
    fn load(&self, section: PackageSectionRef) -> ProviderResult<LegacySnapshotRef>;
}
```

所有 provider 只传 stable id、hash、source span、section ref 和 ABI-safe DTO。旧 VM 指针、平台句柄、Editor widget、商业 payload 和完整脚本文本不得进入 public API。

## Runtime Flow

```text
AstraEMU Manager
  -> create RuntimeWorld
  -> enable family plugin
  -> mount LegacyVfsProvider
  -> compile legacy script into LegacyActionGraph
  -> register StateMachine action provider
  -> tick RuntimeWorld
  -> collect RuntimeEvent / PresentationCommand / AudioCommand / TextCaptureEvent
  -> write LocalCaseReport
```

family plugin 可以持有 family-private interpreter state，但权威推进必须通过 StateMachine action 和可序列化 effect list。每个 effect 在固定 tick 边界进入 Runtime，有 source span、trace id 和 replay hash。

## Family 顺序

v1 可用 family 是 Artemis。后续按通用性排序扩展：KrKr/KAG/TJS、BGI/Ethornell、SoftPAL、FVP、Siglus、Minori。所有 family 复用同一 provider API 和 release gate；私有格式知识留在 family plugin 内，不反向扩展 EngineCore 对象模型。

## Report

Local case report 只包含 hash、coverage、diagnostics、命令、family feature、redaction status 和脱敏 metadata，不包含商业 payload、私有绝对路径、未授权截图、音频采样、provider secret 或可绕过访问控制的说明。

```bash
astra emu probe cases/artemis-synthetic --family artemis --report target/reports/emu-probe.yaml
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
```

Expected report includes `emu.engine_native_family`, `emu.artemis_full_flow`, `emu.report_redaction`, `runtime.replay.determinism` and `plugin.extension_registry`.

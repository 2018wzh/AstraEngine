# Game Runtime Provider Contract

`GameRuntimeProvider` 是 packaged `Game` target 的玩法 runtime 选择层。EngineCore 仍只提供 `RuntimeWorld`、Actor/Component、StateMachine、AwaitToken、Save/Replay、Plugin、Asset/VFS、Media 和 Release Gate；具体玩法由 provider 把产品语义映射成 Runtime action、event、presentation/audio command、package section 和 release check。

这个契约让 AstraVN、AstraEMU 和后续 AstraRPG 成为同级 runtime provider。AstraVN 不作为所有游戏类型的基类；它只实现 VN 语义。AstraEMU 不替换 `RuntimeWorld`；它通过 `AstraEmuRuntimeProvider` 复用 RuntimeWorld，再把旧 VM 交给 family `LegacyRuntimeProvider`。TRPG 不新增 peer provider；桌面规则书玩法落在 `AstraRpgRuntimeProvider` 的 `rpg.trpg` profile/ruleset layer。

## Provider Shape

```rust
pub trait ProductRuntimeProvider: StableProvider {
    fn descriptor(&self) -> ProductRuntimeDescriptor;
    fn prepare(&self, request: RuntimePrepareRequest) -> ProviderResult<RuntimePrepareReport>;
    fn probe(&self, request: RuntimeProbeRequest) -> ProviderResult<RuntimeProbeReport>;
    fn open(&self, request: RuntimeOpenRequest) -> ProviderResult<GameRuntimeSessionId>;
    fn step(&self, session: GameRuntimeSessionId, input: RuntimeStepInput) -> ProviderResult<RuntimeStepOutput>;
    fn save(&self, session: GameRuntimeSessionId, request: RuntimeSaveRequest) -> ProviderResult<RuntimeSaveSections>;
    fn restore(&self, session: GameRuntimeSessionId, request: RuntimeRestoreRequest) -> ProviderResult<RuntimeRestoreReport>;
    fn shutdown(&self, session: GameRuntimeSessionId) -> ProviderResult<RuntimeShutdownReport>;
    fn package_sections(&self, request: RuntimePackageRequest) -> ProviderResult<RuntimePackageSectionPlan>;
    fn release_checks(&self) -> ProviderResult<Vec<ReleaseCheckDescriptor>>;
    fn editor_metadata(&self) -> ProviderResult<RuntimeEditorMetadata>;
}
```

`ProductRuntimeProvider` 注册到 extension registry。项目 target 必须显式绑定 runtime provider、profile 和 package sections；host 不能按插件加载顺序自动选择。Provider 返回的 effect list 由 host adapter 通过 `DeterministicActionContext` 应用，跨插件 ABI 不传递 `RuntimeWorld` 指针。跨插件 ABI 使用 `astra-plugin-abi` 的 `FfiRuntimeProviderRegistration` 和 bounded payload DTO；trait object 不跨 ABI 传递。Host 先显式创建 provider instance，再在该 instance 下管理 session；instance 有活动 session 时必须拒绝销毁。

## Common DTO

```rust
pub struct ProductRuntimeDescriptor {
    pub runtime_id: StableId,
    pub product_kind: ProductKind,
    pub provider_id: ProviderId,
    pub supported_targets: Vec<TargetKind>,
    pub capabilities: Vec<CapabilityId>,
    pub package_sections: Vec<SectionSchemaId>,
    pub release_checks: Vec<ReleaseCheckId>,
}

pub struct RuntimeStepOutput {
    pub status: RuntimeSessionStatus,
    pub effects: Vec<ActionEffect>,
    pub awaits: Vec<AwaitToken>,
    pub presentation: Vec<PresentationCommand>,
    pub audio: Vec<AudioCommand>,
    pub timeline_tasks: Vec<TimelineTask>,
    pub diagnostics: Vec<Diagnostic>,
    pub trace: Vec<StateMachineTrace>,
    pub dirty_save_sections: Vec<SectionId>,
}
```

NativeVN 的 timeline task 通过 `RuntimeOutputDomain::Effect` 和 `astra.vn.timeline_task.v1` 返回。只把 task 写入 `RuntimeWorld` effect trace、却不放入 provider output 属于 `UNWIRED_MAIN_PATH`：Player 无法执行 join/cancel，也不能产生同 run completion evidence。Host 必须先按 descriptor/schema registry 校验该 envelope，再交给 timeline owner；completion 只能在对应 task 真正结束或取消后回到固定 tick 边界。

Player timeline owner 使用 `astra.player_timeline_task.v1` 与 `astra.player_timeline_completion.v1`。Scheduler 必须限制 active task 容量、拒绝重复 task id、非法 symbol、零 duration、未知 cancel 和单调时钟回退；cancel 返回原 start task 的 fence。Windows host 用单调时钟轮询 deadline，并只在 scheduler 产出 completion 后调用 `complete_wait`。同一 provider step 返回的一组 task 要先在临时候选 scheduler 中全部验证，再整体提交，避免中途失败留下部分 active task。

所有 DTO 只能携带 stable id、hash、section ref、`VfsUri`、source span、capability report 和 serde/postcard payload。Luau VM handle、legacy VM object、native renderer/audio handle、Editor widget、local root、provider secret 和商业 payload 不得跨 ABI 或进入 save/replay/report。

## Editor Metadata

`editor_metadata()` 只描述 Editor 可以渲染和调用的作者工具面，不传递 UI widget 或 product runtime 内部对象：

```rust
pub struct RuntimeEditorMetadata {
    pub runtime_id: StableId,
    pub product_kind: ProductKind,
    pub project_templates: Vec<TemplateDescriptor>,
    pub authoring_surfaces: Vec<AuthoringSurfaceDescriptor>,
    pub content_capabilities: Vec<ContentCapabilityDescriptor>,
    pub pie_adapter: Option<PieAdapterDescriptor>,
    pub debug_views: Vec<DebugViewDescriptor>,
    pub release_checks: Vec<ReleaseCheckId>,
    pub source_roundtrip: SourceRoundtripPolicy,
}
```

Editor shell 读取 metadata 后决定 Project Wizard 模板、面板可见性、Content Browser 过滤、PIE adapter、Debugger view 和 Release Gate 跳转。AstraVN 暴露 `.astra` Script、VN Graph、Timeline、System UI 和 Luau policy surface；AstraEMU 只暴露 planned case profile/probe、legacy pack VFS browser、family trace、text/translation overlay、Trusted Luau 和 FilterGraph preset；AstraRPG 暴露 planned Map、Quest、Battle/Party/Inventory、Encounter、Behavior Graph、RPG Inspector、TRPG sheet、seat 和 transcript metadata。

## Peer Runtimes

| Runtime provider | 产品职责 | 当前边界 |
| --- | --- | --- |
| `NativeVnRuntimeProvider` | `.astra` canonical story、VN Core、choice/backlog/save/read-state/voice replay、Luau policy、presentation/system UI、VN package sections 和 VN release checks | 已由 `astra-vn-runtime-provider` 落地；in-process 与 FFI 都执行真实 create/open/step/save/restore/shutdown lifecycle，session 内由 RuntimeWorld StateMachine 的 `astra.vn.step` action 推进；不成为 RPG 或 EMU 的基类 |
| `AstraEmuRuntimeProvider` | legacy case launch、family selection、old VM step bridge、text capture、Trusted Luau patch/decode、FilterGraph preset、local case report 和 EMU release checks | 内部继续使用 family `LegacyRuntimeProvider`；family plugin 不能替换 Runtime tick、Save container 或 Release Gate |
| `AstraRpgRuntimeProvider` | map、party、battle、inventory、quest、encounter、AI behavior、committed output、`rpg.trpg` ruleset/profile 和 RPG-specific editor metadata | planned peer runtime；TRPG 是内部 profile，不是独立 provider；没有现有实现迁移 |

## Runtime Flow

```text
project target
  -> explicit ProductRuntimeProvider binding
  -> prepare/probe package and VFS mounts
  -> open GameRuntime session
  -> RuntimeWorld StateMachine action invokes provider step
  -> host adapter applies serializable effects
  -> save/package/release gate consume provider sections and checks
```

Provider 可以在内部维护 product-specific cursor，但权威 Runtime 结果必须在 fixed tick 边界变成可序列化 effects、await tokens、event queue entries、presentation/audio commands 和 save sections。Replay 读取已保存的 provider output，不重新请求外部 provider 或平台回调。

`ProductRuntimeProvider` 必须显式实现 `create_instance` 和 `destroy_instance`，不得依赖 host 伪造成功报告。产品入口只允许使用 `PackageReader::runtime_provider_selection` 产生的 `ValidatedRuntimeProviderSelection` 创建 `ProductRuntimeHost::bound_in_process`/`bound_ffi`；host 会在 create 前逐字段比对 linked descriptor，在 prepare/probe/open 前比对 target/profile，并验证 provider/runtime report identity。无 package binding 的入口已改名为 `reference_in_process`/`reference_ffi`，只用于明确的 fixture/reference runner，不能进入 Player 或 shipping host。

`ProductRuntimeHost` 还校验 instance/session report identity、首 step 为 `1` 且后续严格连续、output schema/size、save/restore section descriptor、唯一 id、hash 和容量。create、prepare/probe/open 或 duplicate open 的部分成功必须执行 rollback；provider error、panic、malformed output 和 timeout 会 poison 对应 session 或 instance，除 cleanup 外不再接受调用。活动 session 阻断普通 destroy；`cleanup_after_failure` 按 session shutdown 后 destroy，并等待已超时的 blocking provider call drain，不能让后台调用继续并发修改已返回给调用方的 session。

## AstraRPG Profile Boundary

AstraRPG 的完整 contract 见 [AstraRPG Contract](rpg-trpg.md)。项目通过 `runtime_provider: astra_rpg` 选择 provider，再用 profile/ruleset 区分 `traditional_rpg`、`ai_sim` 和 `trpg`。`trpg` profile 的 package/save/report section 必须使用 `rpg.trpg.*`，不能创建顶层 `trpg.*` namespace。CP2020 等规则书适配只能作为 local-private adapter，report 只写 manifest、hash、coverage、byte size 和 diagnostic。

## Release Gate

每个 gameplay runtime 必须声明：

- provider descriptor、engine/rustc/feature fingerprint 和 packaged eligibility。
- required package sections、schema version、hash、codec、migration policy 和 redaction policy。
- scenario runner actions/assertions、route or flow coverage、save/load/replay hash 和 provider-free replay规则。
- Editor metadata 是否可用，以及 metadata 是否能回到同一 public IR。

缺 explicit binding、provider fingerprint 不匹配、package section 不完整、save section 不能迁移、effect 不可序列化、replay 依赖 live provider 或 report 泄露 payload，都必须 blocking。

`runtime_provider.native_vn` 不能只验证 descriptor。Release validator 必须从 `vn.compiled_story` 解码 package payload，执行 open、最短 step、save、restore、state hash 对比和 shutdown，并把 behavior state/event/presentation hash 与 save section count 写入 evidence。FFI lifecycle 由独立测试覆盖 create/destroy、package section open、step、save payload hash、restore 和活动 session 销毁阻断；host lifecycle 测试还必须覆盖 create rollback、duplicate session、step gap、panic、timeout drain、malformed section 和 poisoned cleanup。

# Game Runtime Provider Contract

`GameRuntimeProvider` 是 packaged `Game` target 的玩法 runtime 选择层。EngineCore 仍只提供 `RuntimeWorld`、Actor/Component、StateMachine、AwaitToken、Save/Replay、Plugin、Asset/VFS、Media 和 Release Gate；具体玩法由 provider 把产品语义映射成 Runtime action、event、presentation/audio command、package section 和 release check。

这个契约让 AstraVN、AstraEMU 和后续 AstraRPG 成为同级 runtime provider。AstraVN 不作为所有游戏类型的基类；它只实现 VN 语义。AstraEMU 不替换 `RuntimeWorld`；它通过 `AstraEmuRuntimeProvider` 复用 RuntimeWorld，再把旧 VM 交给 family `LegacyRuntimeProvider`。TRPG 不新增 peer provider；桌面规则书玩法落在 `AstraRpgRuntimeProvider` 的 `rpg.trpg` profile/ruleset layer。

## Provider Shape

```rust
pub trait ProductRuntimeProviderFactory: Send + Sync {
    fn descriptor(&self) -> ProductRuntimeDescriptor;
    fn prepare(&self, request: RuntimePrepareRequest) -> ProviderResult<RuntimePrepareReport>;
    fn probe(&self, request: RuntimeProbeRequest) -> ProviderResult<RuntimeProbeReport>;
    fn open(&self, request: RuntimeOpenRequest) -> ProviderResult<Box<dyn ProductRuntimeSession>>;
    fn package_sections(&self, request: RuntimePackageRequest) -> ProviderResult<RuntimePackageSectionPlan>;
    fn release_checks(&self) -> ProviderResult<Vec<ReleaseCheckDescriptor>>;
    fn editor_metadata(&self) -> ProviderResult<RuntimeEditorMetadata>;
}

pub trait ProductRuntimeSession: Send {
    fn session_id(&self) -> GameRuntimeSessionId;
    fn step(&mut self, input: RuntimeStepInput) -> ProviderResult<RuntimeStepOutput>;
    fn save(&mut self, request: RuntimeSaveRequest) -> ProviderResult<RuntimeSaveSections>;
    fn restore(&mut self, request: RuntimeRestoreRequest) -> ProviderResult<RuntimeRestoreReport>;
    fn shutdown(&mut self) -> ProviderResult<RuntimeShutdownReport>;
}
```

跨插件 ABI 使用 `astra-plugin-abi` 的 `FfiRuntimeProviderRegistration` 及 typed request/result callback；trait object 不跨 ABI 传递。

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
    pub session_id: GameRuntimeSessionId,
    pub status: String,
    pub live: RuntimeLiveOutput,
    pub diagnostics: Vec<String>,
}
```

`live` 是必填的 owned typed output，空 step 返回空集合，不使用 `Option`、缺失值或
fallback。scene、PCM、text、video、timeline、UI、wait、event、blackboard 和 dirty
section 使用各自明确 DTO；旧的通用 payload/control/effect envelope 已删除。实时 step
不生成 persisted output，save、replay、package 和 Evidence 使用各自独立的冷路径契约。

NativeVN timeline task 通过 typed live timeline command 交给 Player owner。Host 完整验证 task id、symbol、duration、join/cancel 和 fence 后再提交；completion 只能在对应 task 真正结束或取消后的下一 fixed tick 回注，不能写入通用 effect trace 或重新编码为 payload。

Player timeline owner 使用 `astra.player_timeline_task.v1` 与 `astra.player_timeline_completion.v1`。Scheduler 必须限制 active task 容量、拒绝重复 task id、非法 symbol、零 duration、未知 cancel 和单调时钟回退；cancel 返回原 start task 的 fence。Windows host 用单调时钟轮询 deadline，并只在 scheduler 产出 completion 后调用 `complete_wait`。同一 provider step 返回的一组 task 要先在临时候选 scheduler 中全部验证，再整体提交，避免中途失败留下部分 active task。

`RuntimeStepInput` 是 product ABI 的完整 tick identity，而不是只有 action 的命令壳：

```rust
pub struct RuntimeStepInput {
    pub session_id: GameRuntimeSessionId,
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: RuntimeStepMode,
    pub input: Option<PlayerInput>,
    pub completions: Vec<AwaitCompletion>,
}
```

Host 在调用 provider 前阻断 step gap/重复/回退、零或超过一秒的 `delta_ns`、seed drift 和 lifecycle mode drift。普通调用只能使用 `Live`；restore 后第一 tick 必须使用一次 `RestoreContinuation`。`Replay` 不允许进入 live provider，provider-free replay 只能读取已固化且 hash 已校验的 recorded output。`RuntimeRestoreReport` 必须返回 `restored_fixed_step` 和 `session_seed`，host 据此恢复连续 tick authority；旧 DTO 缺字段时反序列化直接失败，不提供兼容 fallback。

实时 DTO 只携带 stable id、revision、generation、长度、typed value 和 owned buffer，
不携带 content hash、JSON/postcard payload 或 native handle。Save、replay、package 和
Evidence 使用独立 persisted contract。Luau VM handle、legacy VM object、原生
renderer/audio handle、Editor widget、local root、provider secret 和商业 payload
不得跨 ABI 或进入 save/replay/report。

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
  -> RuntimeWorld atomically commits typed control metadata
  -> owned scene/audio/video allocations move directly to host routing
  -> save/package/release gate consume provider sections and checks
```

Provider 可以在内部维护 product-specific cursor，但 fixed tick 只返回分类后的 typed live output、await token、event、blackboard mutation 与 dirty-section metadata。大块 scene/PCM 不进入 RuntimeWorld transaction；control transaction 成功后直接移动到 host。Replay 消费 transcript 中已记录的 typed ingress/completion，不保存或重放 live provider output，也不重新请求 provider 或平台回调。

`ProductRuntimeProvider` 必须显式实现 `create_instance` 和 `destroy_instance`，不得依赖 host 伪造成功报告。产品入口只允许使用 `PackageReader::runtime_provider_selection` 产生的 `ValidatedRuntimeProviderSelection` 创建 `ProductRuntimeHost::bound_in_process`/`bound_ffi`；host 会在 create 前逐字段比对 linked descriptor，在 prepare/probe/open 前比对 target/profile，并验证 provider/runtime report identity。无 package binding 的入口已改名为 `reference_in_process`/`reference_ffi`，只用于明确的 fixture/reference runner，不能进入 Player 或 shipping host。

`ProductRuntimeHost` 还校验 instance/session report identity、首 step 为 `1` 且后续严格连续、delta/seed/mode、typed sequence、scene/audio bounds、save/restore section descriptor、唯一 id、hash 和容量。live output 不再做 postcard/JSON size traversal；容量由各 typed owner 的元素数、尺寸与字节数直接校验。create、prepare/probe/open 或 duplicate open 的部分成功必须执行 rollback；provider error、panic、malformed output 和 timeout 会 poison 对应 session 或 instance，除 cleanup 外不再接受调用。活动 session 阻断普通 destroy；`cleanup_after_failure` 按 session shutdown 后 destroy，并等待已超时的 blocking provider call drain，不能让后台调用继续并发修改已返回给调用方的 session。

并发 host 使用 factory/session 所有权：factory 必须是 `Send + Sync`，只处理 descriptor、prepare/probe、instance lifecycle 和 session 创建；返回的 `ProductRuntimeSession: Send` 独占其 `RuntimeWorld` 和产品状态。每条 session 有容量 32 的 ordered mailbox，同一 session 永远单飞；不同 session 可以并行。queue full、timeout、panic、provider error、malformed output 和非法 step 只 poison 对应 session；binding、descriptor、package 或 instance control failure才 poison 整个 instance。完整 Headless 多任务测试用独立进程承载平台 session，并通过全局 `WorkerBudgetBroker` 限制 session、text、image、audio、video 和 region worker 的总并发，不能在线程间移动本地 event-loop 对象。

NativeVN product save 只返回一个权威 `runtime.world` section，schema 为 `astra.runtime.save_blob.v4`、codec 为 `Raw`；payload 是 Runtime save container，内部 snapshot 覆盖 StableId generator、Actor/typed Component、StateMachine、Blackboard、Event/Await/delayed queues、MutationLog、mounted module binding 和当前 step。Player save envelope 不再复制一份 `VnRuntimeState`。Restore 必须只接受这一 section，先完成 outer hash、nested container/footer/section hash 和 schema/version 校验，再事务替换 world。旧 save 直接拒绝，不保留迁移器。

## AstraRPG Profile Boundary

AstraRPG 的完整 contract 见 [AstraRPG Contract](rpg-trpg.md)。项目通过 `runtime_provider: astra_rpg` 选择 provider，再用 profile/ruleset 区分 `traditional_rpg`、`ai_sim` 和 `trpg`。`trpg` profile 的 package/save/report section 必须使用 `rpg.trpg.*`，不能创建顶层 `trpg.*` namespace。CP2020 等规则书适配只能作为 local-private adapter，report 只写 manifest、hash、coverage、byte size 和 diagnostic。

## Release Gate

每个 gameplay runtime 必须声明：

- provider descriptor、engine/rustc/feature fingerprint 和 packaged eligibility。
- required package sections、schema version、hash、codec、migration policy 和 redaction policy。
- scenario runner actions/assertions、route or flow coverage、save/load/Evidence digest 和 provider-free typed replay 规则。
- Editor metadata 是否可用，以及 metadata 是否能回到同一 public IR。

缺 explicit binding、provider fingerprint 不匹配、package section 不完整、save section schema 不匹配、typed sequence/bounds 非法、replay 依赖 live provider 或 report 泄露 payload，都必须 blocking。

`runtime_provider.native_vn` 不能只验证 descriptor。Release validator 必须从 `vn.compiled_story` 解码 package payload，执行 open、最短 step、完整 RuntimeWorld save、restore、state hash 对比和 shutdown，并把 behavior state/event/presentation hash 与 save section count 写入 evidence。FFI lifecycle 由独立测试覆盖 create/destroy、package section open、typed step、save payload hash、restore step/seed 和活动 session 销毁阻断；host lifecycle 测试还必须覆盖 create rollback、duplicate session、step gap、delta/seed/mode drift、live-provider replay 阻断、restore continuation、panic、timeout drain、malformed section 和 poisoned cleanup。

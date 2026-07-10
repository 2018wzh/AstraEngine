# Provider And Plugin API Blueprint

插件系统提供机制，不替项目做选择。项目 manifest 必须显式绑定 provider；加载顺序不能改变 runtime 行为。Stage 1 的 `astra-plugin-abi` 持有 ABI-safe structs、`LoadPhase`、extension registry DTO 和 dependency graph DTO；`astra-plugin` 只负责 host loader、registry 和 Runtime action adapter。

## Descriptor

```rust
pub struct PluginDescriptor {
    pub id: PluginId,
    pub version: SemVer,
    pub engine_version: SemVer,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub binary_hash: Hash256,
    pub abi_style: String,
    pub capabilities: Vec<CapabilityId>,
    pub permissions: Vec<PermissionId>,
    pub packaged: bool,
}
```

Release Gate 在加载前校验 descriptor、binary hash、engine version、rustc fingerprint、feature fingerprint、capability、permission、license 和 packaged eligibility。

## Lifecycle

```rust
#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(prefix_ref = AstraPluginModuleRef)))]
pub struct AstraPluginModule {
    pub descriptor_yaml: extern "C" fn() -> RString,
    pub register: extern "C" fn() -> FfiPluginRegistration,
    #[sabi(last_prefix_field)]
    pub shutdown: extern "C" fn() -> FfiPluginShutdown,
}
```

插件通过 `abi_stable::export_root_module` 导出 `AstraPluginModuleRef`；host 使用 `libloading` 打开动态库，再通过 `abi_stable` root module header 完成 layout 和版本校验。插件支持 load/unload，不支持 packaged runtime 内热重载。卸载前 Runtime/Editor 必须停止引用 provider，清空 callback，从 `PluginRegistrar` 删除 slot，并写入 unload report。

Load phase 固定为：

```rust
pub enum LoadPhase {
    EngineBoot,
    ProjectLoad,
    Editor,
    Cook,
    Runtime,
    Package,
    Shutdown,
}
```

插件在每个 phase 只注册该 phase 允许的 extension point。`EditorPanel`、menu command、Inspector widget 只能在 Editor phase 可见；cook processor 只能在 Cook/Package phase 运行；runtime packaged build 会裁剪 editor-only 和 cook-only extension。

## Extension Registry

```rust
pub struct ExtensionPointId(pub StableId);

pub struct PluginDependency {
    pub plugin_id: PluginId,
    pub version_req: VersionReq,
    pub required: bool,
    pub reason: String,
}

pub struct PluginEnablement {
    pub plugin_id: PluginId,
    pub enabled: bool,
    pub scope: EnablementScope,
    pub selected_extensions: Vec<ExtensionPointId>,
}

pub struct ExtensionRegistrationReport {
    pub plugin_id: PluginId,
    pub phase: LoadPhase,
    pub registered: Vec<ExtensionPointId>,
    pub conflicts: Vec<ExtensionConflict>,
    pub dependency_graph: Vec<PluginDependency>,
    pub diagnostics: Vec<Diagnostic>,
}
```

Extension point 覆盖 provider slot、game runtime provider、VFS provider、asset type、importer、cook processor、Editor panel、menu command、graph node、timeline track、Inspector widget、release check 和 legacy family provider。注册必须显式声明 id、phase、capability、permission、packaged eligibility、conflict policy 和 source span。冲突时不使用加载顺序裁决；项目 manifest 或 Plugin Manager 必须选定一个 provider。

Plugin Manager 保存 project enable/disable 状态，读取 Stage 1/2 的 dependency graph，解释缺失依赖、版本冲突、权限不足和 packaged 裁剪原因。Release Gate 输出 `plugin.extension_registry` 和 `plugin.dependency_graph` evidence。

Stage 1 实现 registry backend 和 explicit provider binding。Stage 2 将 `plugin.extension_registry` 与 `plugin.dependency_graph` 写入 package 并执行 release gate。Stage 4 的 Plugin Manager 只提供 Editor UI、enablement 编辑和 diagnostic jump，不再拥有第二套 provider selection 或 dependency graph 实现。

## StateMachine Action Provider

Stage 1 的 gameplay action provider 走 host adapter，不把 trait object 穿过 ABI：

```rust
pub struct FfiPluginRegistration {
    pub providers: RVec<FfiProviderRegistration>,
    pub actions: RVec<FfiActionRegistration>,
    pub callbacks: u32,
}

pub struct FfiActionRegistration {
    pub provider_id: RString,
    pub action_id: RString,
    pub input_schema: RString,
    pub output_schema: RString,
    pub invoke: extern "C" fn(RVec<u8>) -> RVec<u8>,
}
```

`invoke` 的 request/result 都是 postcard 编码的 serde DTO。插件返回 `ActionTrace` 和 `ActionEffect` list；host adapter 通过 `DeterministicActionContext` 应用 effect。插件不接收 `RuntimeWorld`、Actor 指针、Editor widget、GPU/audio native handle 或 platform file descriptor。

卸载插件时，loader 除了清理 `PluginRegistrar` provider slot，还会调用 `RuntimeWorld::unregister_action_provider(provider_id)` 删除该 provider 注册的 action。

## Provider Traits

```rust
pub trait Renderer2DProvider: StableProvider {
    fn descriptor(&self) -> RendererDescriptor;
    fn create(&self, request: RendererCreateRequest) -> ProviderResult<Box<dyn Renderer2D>>;
}

pub trait DecodeProvider: StableProvider {
    fn capability(&self) -> DecodeCapabilityReport;
    fn open(&self, request: DecodeRequest) -> ProviderResult<Box<dyn DecodeStream>>;
}

pub trait AssetImporter: StableProvider {
    fn probe(&self, source: SourceAssetRef) -> ProviderResult<ImportPlan>;
    fn import(&self, plan: ImportPlan) -> ProviderResult<ImportReport>;
}

pub trait CookProcessor: StableProvider {
    fn cook(&self, request: CookRequest) -> ProviderResult<CookArtifact>;
}

pub trait TranslationProvider: StableProvider {
    fn capability(&self) -> TranslationCapabilityReport;
    fn translate_batch(&self, request: TranslationBatchRequest) -> ProviderResult<TranslationBatchResult>;
    fn translate_stream(&self, request: TranslationStreamRequest) -> ProviderResult<OptionalStreamRef>;
}

pub trait AiProvider: StableProvider {
    fn capability(&self) -> ProviderResult<AiCapabilityReport>;
    fn open_session(&self, request: AiSessionRequest) -> ProviderResult<AiSessionId>;
    fn invoke(&self, request: AiInvocationRequest) -> ProviderResult<AiInvocationResult>;
}

pub trait VfsProvider: StableProvider {
    fn capability(&self) -> VfsProviderCapabilityReport;
    fn probe(&self, request: VfsProbeRequest) -> ProviderResult<VfsProbeReport>;
    fn open_mount(&self, request: VfsOpenMountRequest) -> ProviderResult<VfsMountHandle>;
    fn resolve(&self, handle: VfsMountHandle, uri: VfsUri) -> ProviderResult<VfsResolvedEntry>;
    fn read(&self, handle: VfsMountHandle, entry: VfsResolvedEntry, range: ByteRange) -> ProviderResult<BoundedBytes>;
    fn close_mount(&self, handle: VfsMountHandle) -> ProviderResult<VfsCloseReport>;
}

pub trait ProductRuntimeProvider: StableProvider {
    fn descriptor(&self) -> ProductRuntimeDescriptor;
    fn prepare(&self, request: RuntimePrepareRequest) -> ProviderResult<RuntimePrepareReport>;
    fn open(&self, request: RuntimeOpenRequest) -> ProviderResult<GameRuntimeSessionId>;
    fn step(&self, session: GameRuntimeSessionId, input: RuntimeStepInput) -> ProviderResult<RuntimeStepOutput>;
    fn save(&self, session: GameRuntimeSessionId, request: RuntimeSaveRequest) -> ProviderResult<RuntimeSaveSections>;
    fn restore(&self, session: GameRuntimeSessionId, request: RuntimeRestoreRequest) -> ProviderResult<RuntimeRestoreReport>;
    fn shutdown(&self, session: GameRuntimeSessionId) -> ProviderResult<RuntimeShutdownReport>;
}
```

Provider 族还包括 `TextLayoutProvider`、`AudioOutputProvider`、`LuauPolicyBundleProvider`、`EditorPanelProvider`、`AiProvider`、`MCPToolProvider`、`TranslationProvider`、`VfsProvider`、`ProductRuntimeProvider`、`LegacyRuntimeProvider` 和可选 `EMUCoreBridgeProvider`。所有 trait 只传 ABI-safe value、stable id、section ref、`VfsUri` 和 capability report。Stage 3 的 AstraVN 功能 crate 负责公开 VN command、presentation command、Luau policy bundle、Graph/Timeline metadata extension id 和 `NativeVnRuntimeProvider` 绑定；`astra-vn` 只作为 Rust dylib facade 和兼容 re-export，稳定插件边界仍由 `astra-plugin-abi` 承担。

`ProductRuntimeProvider` 的 trait 是 host/in-process 接口。跨 ABI 使用 `astra-plugin-abi` 中的 `FfiRuntimeProviderRegistration`，每个 entrypoint 只收发 bounded `RVec<u8>` DTO payload，并返回统一 `FfiRuntimeProviderResult`。`FfiPluginRegistration.runtime_providers` 注册后，loader 会同时写入现有 provider slot 记录，release gate 继续读取 `provider.policy` 与 `plugin.extension_registry`。NativeVN FFI adapter 已实现显式 instance create/destroy 和真实 session open/step/save/restore/shutdown；活动 session 会阻断 instance destroy。外部 dylib 的分发、签名和跨版本加载仍由后续插件发布工作负责。

VFS backend provider 全部注册到同一个 `vfs_provider` slot。该 slot 允许多个 provider 共存，manifest 的 prefix registry 负责选择 `provider_id`；release gate 只在同一 prefix 缺少明确 provider、provider 未打包、capability mismatch 或依赖 unresolved 时阻断。`game_runtime_provider` 不采用这个多 provider 规则，仍要求目标显式绑定一个玩法 runtime provider。

`AiProvider` 是 Editor 和 MCP host 的后端适配。OpenAI、Ollama、ComfyUI 和 ONNX Runtime 第一方 provider 以普通插件注册，默认禁用，由项目 manifest 或 release profile 显式绑定。Runtime 不直接接收 `AiProvider` trait object，只通过 `McpAiSession` 获取 typed Intent、tool result、generated artifact chunk 和 committed output。

`astra-ai-onnx` provider 可以加载 package/VFS 中声明的 ORT custom op sidecar，但 sidecar 不是新的 Engine extension point。它只作为 ONNX Runtime 依赖被 provider 私有加载，必须由 ModelBundle manifest 声明平台、hash、license、加载策略和目标运行证据。sidecar 不能接收 `RuntimeWorld`、Actor 指针、Editor widget、GPU/audio native handle、provider trait object、platform file descriptor 或本地路径；需要 Engine 能力时必须另行实现普通插件/provider 并走 extension registry。

`TranslationProvider` 是文本翻译专用 slot。`translate_batch` 必须实现；`translate_stream` 只在 provider capability 声明支持时使用。DeepL-style provider 可以只返回 batch 结果，LLM provider 可以 streaming 更新 AstraEMU overlay。翻译结果默认是 UI overlay 状态，不改变 Runtime replay hash。

## Legacy Runtime Provider

```rust
pub struct LegacyFamilyProviderRegistration {
    pub descriptor: LegacyFamilyPluginDescriptor,
    pub runtime: ProviderId,
}
```

AstraEMU family plugin 使用普通 extension registry 注册，不拥有私有 loader 通道。family plugin 注册一个 `LegacyRuntimeProvider` facade；archive reader、旧脚本 VM、media bridge 和 snapshot serializer 都留在 provider session 内。`LegacyRuntimeProvider` 位于 `AstraEmuRuntimeProvider` 之下，不是新的 gameplay runtime selector。Provider 不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

## Game Runtime Provider

`ProductRuntimeProvider` 是 packaged `Game` target 的玩法 runtime selector。NativeVN、AstraEMU 和后续 AstraRPG 都通过这个 slot 显式绑定；AstraVN 不作为所有玩法的基类。Provider 返回的 step output 必须是可序列化 effect list、AwaitToken、presentation/audio command、diagnostic、trace 和 save section ref，由 host adapter 应用到 `DeterministicActionContext`。

`NativeVnRuntimeProvider` 包装现有 AstraVN Core、VN package sections 和 VN release checks。`AstraEmuRuntimeProvider` 包装 Manager/runtime bridge，并在内部选择 family `LegacyRuntimeProvider`。`AstraRpgRuntimeProvider` 只保留同级接入边界，当前不声明已有实现。

## Permissions

```yaml
permissions:
  - id: gpu.surface
    scope: runtime
  - id: filesystem.project_read
    scope: cook
  - id: network.ai_provider
    scope: editor_trusted_session
```

Runtime provider secret、Editor widget、Actor 指针、native GPU/audio handle、platform file descriptor 不能跨 ABI 传递。

## Checks

```bash
cargo test -p astra-plugin descriptor_gate
cargo test -p astra-plugin load_unload
cargo test -p astra-plugin ffi_action_provider
cargo test -p astra-plugin extension_registry
cargo test -p astra-engine dylib_facade
cargo test -p astra-package package_roundtrip
cargo test -p astra-release release_report
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-release plugin_provider_gate
```

Expected report: descriptor mismatch、缺失权限、unload 后 callback、未声明 provider slot、action provider 未清理、extension point conflict、dependency graph 缺失和 packaged 裁剪错误都是 blocking diagnostic。

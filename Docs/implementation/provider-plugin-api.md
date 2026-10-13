# Provider And Plugin API Blueprint

插件系统只提供机制；项目显式绑定实际 provider，不靠加载顺序选择。`astra-plugin-abi` 持有 ABI-safe 注册、load phase、binding 和 dependency DTO，`astra-plugin` 持有 loader 与 registry。Gameplay 与 VN UI 使用静态组合。

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

Extension registry 只维护实际 Rust ABI 的机制注册与依赖，不恢复 `game_runtime_provider`、旧 VN/UI ABI 或 LegacyFamilyPlugin。注册冲突、缺 capability/permission、未 packaged 或 binding mismatch 按实现明确失败。

package 与 Release Validator 消费同一 registry/policy；未来 Editor 界面也不能另写 provider selector 或 dependency graph。目前不把未交付的 Plugin Manager 当作已有功能。

当前 package contract 使用 `astra.plugin_extension_registry.v2` / `astra.provider_policy.v3`。`ProviderBindingContext` 固化 package、target、profile、required capability、engine、rustc、feature 与 ABI fingerprint，`ProviderBinding` 额外记录 canonical SHA-256。Package builder、reader、Release Gate、scenario runner 和 VFS prefix gate 调用同一个 `astra-plugin-abi` validator；policy 与 registry 不能分别解释、按数组第一项选择或在缺 binding 时回退。`presentation` 必须与 policy renderer 相同，`vfs_provider` 必须与 mount backend capability 精确匹配。

## StateMachine Action

Action 是宿主内的 typed Rust 机制，使用 `DeterministicActionContext`，不通过动态 bytes invoke 或 Postcard effect envelope 转发。会话不强制所有产品变更经 FSM action；Runtime 的通用 flat FSM 保持可选，见 [Runtime Contract](../contracts/runtime.md)。

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

```



VFS backend provider 全部注册到同一个 `vfs_provider` slot。该 slot 允许多个 provider 共存，manifest 的 prefix registry 负责选择 `provider_id`；release gate 只在同一 prefix 缺少明确 provider、provider 未打包、capability mismatch 或依赖 unresolved 时阻断。Gameplay 由产品静态组合，不使用 provider slot。

旧 AstraEMU 翻译 slot 与 overlay 路径已被独立 Host 的异步正文替换服务取代，不再属于本插件接口。

## AstraEMU 独立接口

AstraEMU 使用独立 Family ABI 与 Host loader，不注册 Engine extension registry。Family 负责原生 VM、媒体和存档，Host 消费最终帧与 PCM。契约见 [AstraEMU 接口](../contracts/astraemu-ipc.md)；本页的 product provider lifecycle 不适用于 Family。

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

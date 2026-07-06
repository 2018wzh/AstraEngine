# Provider And Plugin API Blueprint

插件系统提供机制，不替项目做选择。项目 manifest 必须显式绑定 provider；加载顺序不能改变 runtime 行为。

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

Extension point 覆盖 provider slot、asset type、importer、cook processor、Editor panel、menu command、graph node、timeline track、Inspector widget、release check 和 legacy family provider。注册必须显式声明 id、phase、capability、permission、packaged eligibility、conflict policy 和 source span。冲突时不使用加载顺序裁决；项目 manifest 或 Plugin Manager 必须选定一个 provider。

Plugin Manager 保存 project enable/disable 状态，构建 dependency graph，解释缺失依赖、版本冲突、权限不足和 packaged 裁剪原因。Release Gate 输出 `plugin.extension_registry` 和 `plugin.dependency_graph` evidence。

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
```

Provider 族还包括 `TextLayoutProvider`、`AudioOutputProvider`、`LuauPolicyBundleProvider`、`EditorPanelProvider`、`AIProvider`、`MCPToolProvider`、`LegacyFamilyPluginProvider` 和可选 `EMUCoreBridgeProvider`。所有 trait 只传 ABI-safe value、stable id、section ref 和 capability report。

## Legacy Family Provider

```rust
pub struct LegacyFamilyProviderRegistration {
    pub descriptor: LegacyFamilyPluginDescriptor,
    pub vfs: Option<ProviderId>,
    pub script: Option<ProviderId>,
    pub action: Option<ProviderId>,
    pub media_mapper: Option<ProviderId>,
    pub snapshot_codec: Option<ProviderId>,
}
```

AstraEMU family plugin 使用普通 extension registry 注册，不拥有私有 loader 通道。family plugin 可以注册 VFS/archive provider、legacy script provider、StateMachine action provider、legacy VM adapter、media mapper 和 snapshot codec；不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

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
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-release plugin_provider_gate
```

Expected report: descriptor mismatch、缺失权限、unload 后 callback、未声明 provider slot、action provider 未清理、extension point conflict、dependency graph 缺失和 packaged 裁剪错误都是 blocking diagnostic。

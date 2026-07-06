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

Provider 族还包括 `TextLayoutProvider`、`AudioOutputProvider`、`LuauPolicyBundleProvider`、`EditorPanelProvider`、`AIProvider`、`MCPToolProvider`、`EMUCoreBridgeProvider`。所有 trait 只传 ABI-safe value、stable id、section ref 和 capability report。

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
cargo test -p astra-release plugin_provider_gate
```

Expected report: descriptor mismatch、缺失权限、unload 后 callback、未声明 provider slot 都是 blocking diagnostic。

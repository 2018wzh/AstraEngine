# Plugin ABI Contract

Astra 插件采用 Rust-facing `abi_stable` 风格 ABI。目标是给插件作者 Rust 体验，同时让二进制兼容性可检查、可拒绝、可审计。

## 版本绑定

插件必须声明：

```yaml
id: com.example.renderer.wgpu_plus
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-1.87.0-stable-x86_64-pc-windows-msvc
feature_fingerprint: astra-runtime+wgpu+serde-2026-07
abi_style: abi_stable_rust
capabilities:
  - renderer2d.provider
permissions:
  - gpu.surface
packaged: true
```

Release Gate 在加载前校验 descriptor、binary hash、engine version、rustc fingerprint、feature fingerprint、capability、permission、packaged eligibility 和依赖闭包。

## Entry Point

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

插件用 `abi_stable::export_root_module` 导出 `AstraPluginModuleRef`。`libloading` 负责打开动态库，loader 读取 `abi_stable` root module header 并校验 layout，再取 descriptor、register 和 shutdown 函数。SDK 可以用 Rust trait 包装 entry，但稳定边界仍以 descriptor YAML 和 ABI-safe value 为准。插件可以加载和卸载，不支持运行中重载。需要替换版本时，Runtime/Editor 必须完成 unload，再加载新 binary。

## Provider 注册

Provider 通过 ServiceRegistry、ExtensionRegistry 和 EngineModuleSlot 注册。Provider 不返回 host-owned native handle，不保存跨 unload callback，不暴露 Editor widget 或内部 Actor 指针。

```rust
pub trait Renderer2DProvider: StableProvider {
    fn descriptor(&self) -> RendererDescriptor;
    fn create_device(&self, request: RendererCreateRequest) -> Result<Box<dyn Renderer2D>, ProviderError>;
}
```

完整 provider trait 族见 [Provider And Plugin API Blueprint](../implementation/provider-plugin-api.md)。v1 必须覆盖 Renderer2D、TextLayout、AudioOutput、DecodeProvider、AssetImporter、CookProcessor、LuauPolicyBundle、EditorPanel、AIProvider、MCPToolProvider 和 EMUCoreBridge。

## Load / Unload Report

```yaml
schema: astra.plugin_report.v1
plugin: com.example.renderer.wgpu_plus
status: unloaded
registered_slots: [renderer2d]
callbacks_released: true
opaque_sections_registered: []
diagnostics: []
```

卸载后仍有 callback、opaque state 未注册 migrator、capability 与 permission 不匹配，都是 blocking diagnostic。

## Luau Policy Bundle

复杂演出插件采用 Rust 机制、Luau 策略。Rust 插件声明 provider、native node 和 capability；Luau policy bundle 声明 command schema、hook、mutation scope、Editor metadata、preview、performance budget、save migrator 和 release check。项目 manifest 必须显式绑定 command/preset provider，不按加载顺序抢占。

开发期可以通过 pesde 解析 Luau 依赖；Package 阶段必须生成 lock/vendor cache。Release Gate 校验依赖 hash、license、capability、schema、migrator 和 Luau snapshot policy。

## derive 宏

derive 宏可以生成 descriptor、schema、Inspector、save/replay、MCP patch glue 和注册样板。宏必须支持 `cargo expand` 调试路径，不得生成隐藏继承、全局对象系统或不可见生命周期。

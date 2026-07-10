# Plugin ABI Contract

Astra 插件采用 Rust-facing `abi_stable` 风格 ABI。目标是给插件作者 Rust 体验，同时让二进制兼容性可检查、可拒绝、可审计。

`astra-plugin-abi` 是插件二进制边界的真源，包含 `AstraPluginModule`、FFI registration、`LoadPhase`、extension registry DTO、dependency graph DTO 和 runtime provider DTO+FFI entrypoints。`astra-plugin` 是 host 侧 loader、registry 和 Runtime action adapter。`astra-engine` 和 `astra-vn` 都是 Rust dylib facade，分别 re-export EngineCore 与 VN public API，不扩大插件 ABI。

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

完整 provider trait 族见 [Provider And Plugin API Blueprint](../implementation/provider-plugin-api.md)。v1 必须覆盖 Renderer2D、TextLayout、AudioOutput、DecodeProvider、AssetImporter、CookProcessor、LuauPolicyBundle、EditorPanel、AiProvider、MCPToolProvider、GameRuntimeProvider、LegacyFamilyPlugin 和可选 EMUCoreBridge。AiProvider 只服务 Editor 和 MCP host，Runtime 通过 `McpAiSession` 消费固化后的 Intent 和 output。

## Game Runtime Provider ABI

`ProductRuntimeProvider` 是 Rust 侧设计 trait；跨插件 ABI 不传 trait object。插件通过 `FfiRuntimeProviderRegistration` 注册 ABI-safe entrypoints，并继续把 provider 记录写入现有 `providers` slot snapshot，让 package/release gate 复用同一 registry：

```rust
pub struct FfiRuntimeProviderRegistration {
    pub provider_id: RString,
    pub runtime_id: RString,
    pub descriptor_json: RString,
    pub descriptor_schema: RString,
    pub capability: RString,
    pub phase: LoadPhase,
    pub packaged: bool,
    pub prepare: FfiRuntimeProviderInvoke,
    pub probe: FfiRuntimeProviderInvoke,
    pub open: FfiRuntimeProviderInvoke,
    pub step: FfiRuntimeProviderInvoke,
    pub save: FfiRuntimeProviderInvoke,
    pub restore: FfiRuntimeProviderInvoke,
    pub shutdown: FfiRuntimeProviderInvoke,
    pub package_sections: FfiRuntimeProviderInvoke,
    pub release_checks: FfiRuntimeProviderInvoke,
    pub editor_metadata: FfiRuntimeProviderInvoke,
}
```

每个 entrypoint 接收 bounded `RVec<u8>` payload，内容为 serde JSON 或 postcard encoded runtime provider DTO，返回 `FfiRuntimeProviderResult { ok, payload, diagnostics }`。`FfiPluginRegistration.runtime_providers` 可以包含多个 runtime provider registration，但 `game_runtime_provider` slot 在 target 绑定时仍要求单 provider 显式选择。`vfs_provider` 的多 provider 同 slot 规则不适用于 gameplay runtime。

当前常量包括 `GAME_RUNTIME_PROVIDER_SLOT = "game_runtime_provider"`、`NATIVE_VN_RUNTIME_ID = "native_vn"` 和 `NATIVE_VN_PROVIDER_ID = "astra.runtime.native_vn"`。NativeVN FFI adapter 提供真实 provider instance create/destroy 与 session open/step/save/restore/shutdown；请求和返回只携带 bounded serialized DTO。外部 dylib 的分发、签名和跨版本加载不作为本阶段完成边界。

FFI 边界只传 stable id、string id、section ref、hash、diagnostic 和 bounded payload。Luau VM handle、RuntimeWorld 指针、Editor widget、本地 root、provider secret、商业文本、音频、图像或 bytecode payload 都不得跨 ABI。

## Extension Registry

插件可以注册 provider slot、asset type、importer、cook processor、Editor panel、menu command、graph node、timeline track、Inspector widget、release check、game runtime provider 和 legacy family provider。每条注册必须声明 `LoadPhase`、依赖、冲突策略、enablement scope、packaged eligibility 和 diagnostic source。Editor 的 Plugin Manager 只能改变 enablement 和 binding，不能绕过 descriptor gate。

Stage 1 host registry 负责 `LoadPhase`、provider binding、conflict report 和 `PluginDependency` dependency graph。Stage 2 package/release gate 写入并校验 `plugin.extension_registry` 和 `plugin.dependency_graph`。Stage 4 Editor Plugin Manager 只显示和修改这些后端产物。

## StateMachine Action ABI

Stage 1 action provider 使用 function pointer 和 postcard payload：

```rust
pub struct FfiActionRegistration {
    pub provider_id: RString,
    pub action_id: RString,
    pub input_schema: RString,
    pub output_schema: RString,
    pub invoke: extern "C" fn(RVec<u8>) -> RVec<u8>,
}
```

`invoke` 接收 `ActionCallRequest` bytes，返回 `ActionCallResult` bytes。result 只包含 `ActionTrace` 和可序列化 `ActionEffect` list；host adapter 负责应用 effect。插件不得跨 ABI 接收或保存 RuntimeWorld、Actor 指针、trait object、native handle 或平台文件描述符。

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

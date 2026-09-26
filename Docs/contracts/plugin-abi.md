# Plugin ABI Contract

Astra 插件采用 Rust-facing `abi_stable` 风格 ABI。目标是给插件作者 Rust 体验，同时让二进制兼容性可检查、可拒绝、可审计。

`astra-plugin-abi` 保留机制 provider 的 registry、binding、dependency DTO 和 `AstraPluginModule` 注册边界。`ffi`/`dynamic-abi` 仅用于原生动态插件加载。Gameplay 由产品 binary 静态组合；NativeVN 直接持有 `VnSession`，不再有动态 gameplay descriptor、host 或 FFI session lifecycle。Rust live presentation 类型暂留于此 crate，供现有静态消费者使用，不序列化为插件 payload。

动态制品边界见 [重构契约](rebuild.md)。

```bash
python Tools/check_dynamic_artifacts.py
```

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

完整 provider trait 族见 [Provider And Plugin API Blueprint](../implementation/provider-plugin-api.md)。v1 必须覆盖 Renderer2D、TextLayout、AudioOutput、DecodeProvider、AssetImporter、CookProcessor、LuauPolicyBundle、EditorPanel、AiProvider、MCPToolProvider；EMU Family 使用独立 ABI。AiProvider 只服务 Editor 和 MCP host，Runtime 通过 `McpAiSession` 消费固化后的 Intent 和 output。

## 静态产品组合

`astra.target_manifest.v3` 不再要求 `runtime_provider` 或 `ui_provider`；`astra.provider_policy.v3` 只声明实际机制绑定。旧 schema、旧 descriptor 字段和 `game_runtime_provider` slot 均拒绝，项目需重新 Cook。`PackageProductContext` 只提供已验证的 target/profile；包完整性、VFS、renderer、audio 和 decode 的显式选择保持不变。发布检查直接验证 NativeVN section 与实际 session 保存恢复流程，不消费插件自声明的 release checks。

## Extension Registry

插件可以注册 provider slot、asset type、importer、cook processor、Editor panel、menu command、graph node、timeline track、Inspector widget、release check。每条注册必须声明 `LoadPhase`、依赖、冲突策略、enablement scope、packaged eligibility 和 diagnostic source。Editor 的 Plugin Manager 只能改变 enablement 和 binding，不能绕过 descriptor gate。

Package 使用 `astra.plugin_extension_registry.v2` 和 `astra.provider_policy.v3`。两者共享 `ProviderBinding`，binding 同时固化 slot、provider id、required capability、package、target、profile、engine version、rustc/feature/ABI fingerprint 与 canonical `binding_hash`。Registry 中的 binding 集合必须与 policy 完全一致；缺失、重复、冲突、hash drift、capability/fingerprint mismatch、未 packaged provider 或 target/profile 不一致都会在 package builder、package reader、scenario runner 和 Release Gate 的同一 typed validator 中阻断。v1 section 不迁移，也不自动补默认 provider。

`PackageReader` 保留已验证的 `PackageProductContext` 和包 SHA-256。产品从 target/profile 创建静态 session，作品身份与内容校验继续用于保存恢复。

`PluginRegistrar::register_provider` 是 fallible API。非法 identity 与重复 `(slot, provider_id)` 不写入 registry；loader 先解析全部 registration，再提交注册，任一注册失败会撤销本次已提交的 provider。无 binding context 的选择 API 不存在，provider 注册顺序不能改变结果。

Stage 1 host registry 负责 `LoadPhase`、provider binding、conflict report 和 `PluginDependency` dependency graph。Stage 2 package/release gate 写入并校验 `plugin.extension_registry` 和 `plugin.dependency_graph`。Stage 4 Editor Plugin Manager 只显示和修改这些后端产物。

## StateMachine Action

Action 是 host 内注册的 typed Rust action，通过 `DeterministicActionContext` 修改运行状态；不提供动态 bytes invoke 或 effect envelope。

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

# Plugin Developer Guide

插件通过 Rust-facing `abi_stable` 风格 ABI 接入。插件可以提供 renderer、text layout、audio、decode、script runtime、presentation library、asset importer、cook processor、editor panel、menu command、graph node、timeline track、Inspector widget、release check、MCP tool、AI provider、LegacyFamilyPlugin 或 Luau policy bundle 的 native 机制。

## 插件文件

```text
plugin.yaml
src/lib.rs
policy.luau
tests/load_unload.rs
manual.md
```

`plugin.yaml` 必须声明 id、version、engine version、rustc fingerprint、feature fingerprint、capability、permission、load phase、extension point、dependency graph、enablement scope 和 packaged eligibility。Luau policy bundle 还要声明命令 schema、Editor metadata、hook、mutation scope、save migrator、performance budget 和依赖锁定策略。

## 禁止项

插件不得跨 ABI 保存 host object ownership、Actor 指针、GPU/audio native handle、Editor widget 或 unload 后 callback。需要 runtime state 时，注册 save section 和 migrator。

复杂演出插件采用 Rust 机制、Luau 策略。Rust 侧提供高性能 node/provider/host API；Luau 侧决定时序、参数、fallback、预设和可视化元数据。

## Extension Reference

插件可实现 Renderer2D、TextLayout、AudioOutput、DecodeProvider、AssetImporter、CookProcessor、LuauPolicyBundle、EditorPanel、AIProvider、MCPToolProvider、LegacyFamilyPlugin 或可选 EMUCoreBridge。descriptor、permission、load/unload、provider trait、extension registry 和 report 见 [Provider And Plugin API Blueprint](../implementation/provider-plugin-api.md)。

Plugin Manager 显示启用状态、依赖链、冲突、权限、packaged 裁剪和 diagnostic jump。插件不能依赖加载顺序抢占 provider；项目 manifest 必须显式绑定命令、provider 和 release check。

## 验收命令

```bash
cargo test -p astra-plugin descriptor_gate
cargo test -p astra-plugin load_unload
cargo test -p astra-plugin extension_registry
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-release plugin_provider_gate
```

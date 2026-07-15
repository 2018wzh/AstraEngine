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

插件只发 `tracing` span/event，不安装 subscriber 或自建日志文件。事件使用稳定 plugin/provider category 和 `event` 字段：load/unload、provider register/session lifecycle 用 `INFO`，选择和映射用 `DEBUG`，高频调用用 `TRACE`，允许继续的明确 fallback 用 `WARN`，最终拒绝加载或 ABI/provider 操作失败由 host 边界记录一次 `ERROR`。字段只能包含 plugin/provider/action id、fingerprint hash、状态、计数和 diagnostic code；不得记录 payload、secret、native handle、绝对路径或完整 descriptor `Debug` 输出。

复杂演出插件采用 Rust 机制、Luau 策略。Rust 侧提供高性能 node/provider/host API；Luau 侧决定时序、参数、fallback、预设和可视化元数据。

## Extension Reference

插件可实现 Renderer2D、TextLayout、AudioOutput、DecodeProvider、AssetImporter、CookProcessor、LuauPolicyBundle、EditorPanel、AiProvider、MCPToolProvider、LegacyFamilyPlugin 或可选 EMUCoreBridge。descriptor、permission、load/unload、provider trait、extension registry 和 report 见 [Provider And Plugin API Blueprint](../implementation/provider-plugin-api.md)。

作品专属 UI component 使用独立 `astra-ui-plugin-abi`，不能通过通用 widget callback 或 Yakui object 穿过 ABI。组件只能挂到 `.astra` 静态 typed slot；Windows dylib 由独立组件子进程加载并要求 Ed25519 signer allowlist，Web component 要求已校验 WIT/jco output。组件失败会终止 UI session，不会替换 provider 或生成替代组件。完整边界和 hard limit 见 [UI Component Plugin Contract](../contracts/ui-component-plugin.md)。

AiProvider 只服务 Editor 和 MCP host。OpenAI、Ollama、ComfyUI、ONNX Runtime 这类 provider 必须声明 capability、secret handle、data egress、debug trace policy、runtime eligibility 和真实 smoke opt-in。Runtime 不能直接持有 provider，只能通过 `McpAiSession` 消费 typed Intent、generated artifact chunk 和 committed output。

项目自管 ORT custom op sidecar 时，sidecar 只能作为 ModelBundle 的 Asset VFS content entry 被 `astra-ai-onnx` 私有加载。它必须声明平台二进制、hash、license、加载策略和目标运行证据，不能保存或暴露 host object ownership、Actor 指针、RuntimeWorld、Editor widget、GPU/audio native handle、provider trait object 或本地路径。需要访问 Engine 能力时，写普通插件/provider 并走 extension registry。

Plugin Manager 显示启用状态、依赖链、冲突、权限、packaged 裁剪和 diagnostic jump。插件不能依赖加载顺序抢占 provider；项目 manifest 必须显式绑定命令、provider 和 release check。

## 验收命令

```bash
cargo test -p astra-plugin descriptor_gate
cargo test -p astra-plugin load_unload
cargo test -p astra-plugin extension_registry
cargo test -p astra-ai provider_profiles
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-release plugin_provider_gate
```

# Plugin Runtime Boundary Migration

本计划迁移已经存在的 plugin registry、StateMachine action provider 和 VN extension fixture 文档口径，使它们支持单一 `vfs_provider` slot 与 gameplay runtime provider selection。它不新增 AstraEMU family plugin 实现，也不迁移尚未存在的 AstraRPG provider。

## 现有实现入口

- `Engine/Source/Runtime/astra-plugin-abi`：ABI-safe root module、load phase、FFI DTO。
- `Engine/Source/Runtime/astra-plugin`：descriptor gate、loader、extension registry、dependency graph、action adapter 和 unload cleanup。
- `Engine/Plugins/Fixtures/headless-presentation-provider`：Stage 1 presentation/action provider fixture。
- `Engine/Plugins/Fixtures/vn-extension-provider`：VN policy/command/presentation/editor metadata/release check provider fixture。
- `Engine/Source/Runtime/astra-package` 和 `Engine/Source/Developer/astra-release`：`plugin.extension_registry`、`plugin.dependency_graph` package section 与 release gate。

## 目标设计

Plugin registry 继续只提供机制，不替项目选择 provider。新增口径：

- `VfsProvider`：package、local authorized、legacy pack、overlay 和 memory backend provider，统一注册到 `vfs_provider` slot。
- `ProductRuntimeProvider`：`NativeVnRuntimeProvider`、`AstraEmuRuntimeProvider` 和后续 `AstraRpgRuntimeProvider` 的 gameplay runtime selector。
- `LegacyRuntimeProvider`：仍只是 AstraEMU family provider，位于 `AstraEmuRuntimeProvider` 之下。

所有 provider 都必须通过 explicit binding、descriptor fingerprint、permission、capability、packaged eligibility 和 release gate。加载顺序不能决定 runtime provider、VFS prefix provider 或 VN extension provider。`vfs_provider` slot 允许多个 provider 并存；manifest prefix registry 负责选择 `provider_id`。`game_runtime_provider` 仍要求单 provider 显式绑定。

## 分步迁移

1. 扩展 extension registry schema。
   增加 `game_runtime_provider` 和 `vfs_provider` extension point id，保留现有 action、presentation、VN extension 和 release check ids。
2. 调整 provider policy binding。
   Project target 显式绑定 gameplay runtime provider；`asset.vfs_manifest.prefixes` 显式绑定 VFS prefix provider；VN extension 继续显式绑定 Luau policy、command、presentation、metadata 和 release check provider。
3. 更新 package registry sections。
   `plugin.extension_registry` 和 `plugin.dependency_graph` 增加新 extension point 类型、packaged eligibility 和 conflict policy。
4. 更新 release gate。
   缺 gameplay runtime provider、缺 VFS prefix provider、provider capability mismatch、provider conflict、missing dependency、packaged trim 错误或权限不足都 blocking。
5. 迁移 fixture。
   `vn-extension-provider` 继续证明 VN extension slots；新增 runtime provider fixture 时只覆盖 `NativeVnRuntimeProvider` selection，不把 AstraEMU/AstraRPG 写成已有 fixture。
6. 更新 unload cleanup。
   卸载插件时清理 action provider、VFS provider、gameplay runtime provider 和 VN extension provider 注册项，不能留下 callback 或 stale binding。

## 验收命令

```bash
python Tools/check_docs.py
cargo test -p astra-plugin descriptor_gate
cargo test -p astra-plugin load_unload
cargo test -p astra-plugin ffi_action_provider
cargo test -p astra-plugin extension_registry
cargo test -p astra-plugin vfs_provider_registry
cargo test -p astra-plugin runtime_provider_registry
cargo test -p astra-package package_roundtrip
cargo test -p astra-release release_report
cargo test -p astra-vn-plugin --test vn_plugin_extensions
```

`astra-release plugin_provider_gate` 仍是后续专门 gate 名；当前 VFS prefix/provider binding 已由 `cargo test -p astra-release vfs_mount_gate` 覆盖。

## 不得修改项

- 不引入第二套 plugin manager、dependency graph 或 provider selector。
- 不允许 provider 接收 `RuntimeWorld` 指针、Actor 指针、Editor widget、native GPU/audio handle、platform file descriptor、本地 root 或商业 payload。
- 不把 `LegacyRuntimeProvider` 升级为顶层 gameplay runtime selector。
- 不把 VN extension fixture 写成 AstraEMU/AstraRPG 已实现证据。

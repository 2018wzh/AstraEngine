# Migration Plans

本目录只记录已实现代码向新设计对齐的迁移路线。设计页可以覆盖完整未来架构；迁移页不能把尚未存在的 AstraEMU/AstraRPG 代码写成可搬迁对象。

## 执行顺序

| 顺序 | 文档 | 范围 |
| --- | --- | --- |
| 1 | [asset-vfs-migration.md](asset-vfs-migration.md) | 现有 `astra-asset`、`astra-cook`、`astra-package`、旧 asset registry writer、package section、TsuiNoSora mount/direct-read evidence 对齐到 Provider URI Asset VFS |
| 2 | [plugin-runtime-boundary-migration.md](plugin-runtime-boundary-migration.md) | 现有 plugin registry、action provider、VN extension fixture 对齐到单一 `vfs_provider` slot 与 gameplay runtime provider selection |
| 3 | [astra-vn-module-layout-migration.md](astra-vn-module-layout-migration.md) | 现有 `astra-vn` 从 Runtime 分区迁到 `Engine/Source/Modules/AstraVN/astra-vn` |
| 4 | [astra-vn-crate-split-migration.md](astra-vn-crate-split-migration.md) | 现有单 crate `astra-vn` 拆成 AstraVN 多功能 crate，`astra-vn` 收缩为 facade |
| 5 | [game-runtime-provider-migration.md](game-runtime-provider-migration.md) | 现有 AstraVN runtime facade、VN extension manifest、package sections、release checks 对齐到 `NativeVnRuntimeProvider` |
| 6 | [editor-runtime-provider-migration.md](editor-runtime-provider-migration.md) | 现有 Editor 设计、手册和状态口径对齐到 runtime-provider-aware shell |

## 范围边界

迁移已有实现：

- `astra-asset` 的 `AssetId`、sidecar、`VfsUri`、manifest/catalog DTO 和 path policy。
- `astra-cook` 的 importer/cook artifact、NativeVN asset sidecar 和 cook audit。
- `astra-package` 的 package/save container、section table、bounded reader、`asset.vfs_manifest`、`asset.catalog` 和 project-level `package_sections`。
- `astra-vn` 的 module layout、facade、VN state/save、VN extension manifest、package sections 和 release checks。
- Stage 1 plugin registry、StateMachine action provider 和 `vn-extension-provider` fixture。
- Editor workflow、module、creator manual 和 Stage 4 状态文档中的 runtime provider switching 口径。
- TsuiNoSora 的脱敏 mount policy、`mount_probes`、route-bound `mount_assets`、NativeVN asset sidecar/cooked asset/package VFS manifest/catalog 和 player route evidence。

后续新实现：

- `AstraEmuRuntimeProvider`、AstraEMU Manager、family `LegacyRuntimeProvider` 代码、legacy pack VFS reader 和 EmulatorCore 状态机映射。
- `AstraRpgRuntimeProvider` 和 RPG 专属 gameplay runtime。
- Stage 4 AI/MCP provider 代码、ONNX provider 代码和 Editor UI。
- AstraEMU/AstraRPG runtime provider 代码和专属 Editor surface；它们只在设计页和状态页保留 planned peer runtime 边界。

## 验收命令

```bash
python Tools/check_docs.py
```

代码迁移完成后，再按对应阶段追加 Rust 测试、release gate 和 scenario 验收。文档迁移不能把未跑过的命令写成 `DONE` evidence。

# Asset VFS Migration

本计划迁移已经存在的 Asset/Cook/Package/TsuiNoSora 资产证据，使其对齐 [Asset VFS Contract](../contracts/asset-vfs.md)。本页不实现 legacy pack reader，也不把旧引擎 pack 当作 `.astrapkg` 替代品。

## 现有实现入口

- `Engine/Source/Runtime/astra-asset`：`AssetId`、`AssetSidecar`、`AssetRegistry`、source path validation 和 sidecar schema tests。
- `Engine/Source/Developer/astra-cook`：importer、cook processor、DDC key、cook audit、NativeVN asset sidecar 输入。
- `Engine/Source/Runtime/astra-package`：package/save container、section table、Zstd/Raw/Postcard codec、bounded reader、plugin registry section 和 project-level `package_sections`。
- `Engine/Source/Programs/astra-cli`：`astra cook`、`astra package build`、`astra package validate`、NativeVN sample/package/bundle 流程。
- `Tools/TsuiNoSora`：脱敏 inventory、Asset analysis、conversion report、mount policy、`mount_probes`、route-bound `mount_assets` 和 NativeVN package input report。
- `Engine/Source/Developer/astra-release`：package integrity、TsuiNoSora section redaction、mount policy、conversion manifest 和 player route checks。

## 目标设计

`astra-asset` 成为 VFS contract owner，输出：

- `VfsMountDescriptor`：package、local authorized、legacy pack、overlay 四类 mount。
- `VfsLocator`：alias、relative key、expected hash、media kind。
- `VfsResolvedEntry`：mount id、source ref、entry ref、offset、size、hash、codec、media kind、diagnostic。
- `VfsResolveReport`：只记录 alias、relative key、pack/entry、offset、size、hash、media kind 和 diagnostic。

`astra-package` 只实现 package-backed mount。`project_sections` 继续只适合脱敏 report/manifest，不写商业 payload、模型 payload、截图、音频、影片或本地 root。

## 分步迁移

1. 在 `astra-asset` 增加 VFS DTO 和 schema。
   保留 `AssetId`、sidecar 和 registry public API；新 DTO 先与旧 registry 并存，避免一次性改动 cook/package。
2. 把现有 `asset.registry.assets` 转为 package mount locator。
   每条 cooked asset 记录 section id、relative key、role、hash、byte size 和 media kind；旧字段继续读，writer 增加 VFS projection。
3. 把 `astra-package` reader 包装为 package-backed VFS mount。
   Package reader 仍只校验 section table、codec、offset、size 和 hash；VFS 层负责 alias/key/report。
4. 把 `nativevn.asset_roots` 和 TsuiNoSora `mount_probes`/`mount_assets` 改为 local authorized mount descriptor。
   Report 继续只写 alias、relative path、role、route id、hash、byte size 和 diagnostic。
5. 把 project-level `package_sections` 接入 VFS redaction gate。
   进入 package 的 JSON section 继续清洗 payload-like 字段；违规字段只记录字段路径和 diagnostic。
6. 增加 overlay mount gate。
   先覆盖 synthetic patch/direct-read route；没有 allowlist 的同 key 多命中必须 blocking。
7. 追加 release report 字段。
   增加 `vfs.package_mount`、`vfs.local_authorized_mount`、`vfs.overlay_mount`；`vfs.legacy_pack_mount` 先作为后续 AstraEMU 依赖，不要求现有代码通过。

## 验收命令

```bash
python Tools/check_docs.py
cargo test -p astra-asset sidecar_schema
cargo test -p astra-cook import_cook
cargo test -p astra-package package_roundtrip
cargo test -p astra-release --test release_report tsuinosora
cargo test -p astra-cli --test target_platform tsuinosora_synthetic_gate_runs_internal_and_patch_player_routes
```

新增 VFS 代码后再补：

```bash
cargo test -p astra-asset vfs_mount_descriptor
cargo test -p astra-package package_vfs_mount
cargo test -p astra-release vfs_mount_gate
```

## 不得修改项

- 不改 `.astrapkg` container header、section table、hash、codec 和 save/package 共用 container 语义。
- 不把 legacy pack reader 写进 `astra-package`。
- 不把本地 root、绝对路径、商业 payload、截图、音频、影片、bytecode 或完整脚本文本写入 package、save 或 report。
- 不删除现有 AssetId、sidecar、registry、NativeVN asset sidecar 和 TsuiNoSora 脱敏 evidence。

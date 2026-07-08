# Asset VFS Blueprint

本页把 [Asset VFS Contract](../contracts/asset-vfs.md) 落到 workspace 边界。VFS 不是另一个 package 容器，也不是文件系统直通层；它是 `astra-asset` 拥有的统一读源、权限、hash 和证据模型。public locator 统一使用 `provider:/path/file` 风格的 `VfsUri`。

## Ownership

| Crate | 职责 |
| --- | --- |
| `astra-asset` | `VfsUri`、prefix descriptor、provider descriptor、backend config、layer graph、entry table、whiteout、`asset.catalog` DTO、local root capability 和 bounded reader |
| `astra-cook` | 把 import source、sidecar、native asset roots、ModelBundle source 和 project package section 转成 package VFS entry 输入 |
| `astra-package` | 写入 `asset.vfs_manifest` 和 `asset.catalog`，并提供 package-backed section source；它不解释 VN、AI、EMU 或 legacy pack 语义 |
| `astra-plugin` | 在保留 `slot: String` ABI 的前提下，让多个 provider 注册到单一 `vfs_provider` slot；`game_runtime_provider` 仍保持单 provider 绑定 |
| `astra-release` | 校验 URI、prefix registry、provider binding、package bounds/hash、overlay/whiteout、catalog 引用和 redaction |

## Manifest Shape

当前 writer 生成的 release package 必须包含：

- `asset.vfs_manifest`：`schema`、`prefixes`、`layers`、`entries` 和 `whiteouts`。
- `asset.catalog`：`schema` 和 gameplay/editor 可见的 `assets` rows。

`asset.registry` 已退出 package 内真源；release gate 看到该 section 必须 blocking。源码侧 `AssetId`、sidecar 和 cook artifact API 可以继续存在，但它们只是 VFS manifest/catalog 的输入。

## Mount Resolution

VFS resolve 的输入是 `(target, profile, vfs_uri, expected_hash, media_kind)`。Host 先按 target/profile 选出 layer graph，再按固定 priority 查找：

1. overlay layer：同一 `VfsUri` namespace 下覆盖 lower layer，whiteout 必须有 allowlist、base hash 和 reason。
2. package layer：读取 `.astrapkg` section 或 patch package section。
3. legacy pack layer：由 family provider 解析 pack entry table，Stage 5 才落地 reader。
4. local authorized layer：只服务开发期、私有 acceptance 或用户显式授权 case。
5. memory/workspace layer：只用于 Editor/Tools 临时对象，不进入 shipping package。

命中结果必须携带 `vfs_uri`、source ref、entry ref、offset、size、hash、codec、media kind 和 diagnostic。读取 bytes 前校验 bounds，读取后校验 hash。多 layer 冲突没有 overlay policy 时阻断。

## Provider Binding

`plugin.extension_registry.providers` 继续使用现有 `FfiProviderRegistration { slot, provider_id, capability, phase, packaged }` 形状。VFS 只约定一个 slot：

```text
vfs_provider
```

manifest 中每个 prefix 都通过 `provider_id` 绑定一个 provider。release gate 检查 provider 已注册、`packaged: true`、capability 与 backend 匹配，例如 `vfs.backend.package`、`vfs.backend.local_authorized`、`vfs.backend.overlay`、`vfs.backend.legacy_pack.fvp`。多个 `vfs_provider` 可以共存；manifest 负责选择 prefix/provider，不能靠加载顺序猜。

## Package Relationship

`.astrapkg` 保留为控制面和证据面容器。它保存 target manifest、plugin registry、provider policy、compiled story、VN/AI/EMU package sections、release summary、`asset.vfs_manifest`、`asset.catalog` 和 cooked assets。旧引擎 pack 不替代 `.astrapkg`；它只作为 `legacy_pack` backend 被 AstraEMU 或迁移工具读取 payload。

典型组合：

- NativeVN release：`package:/native-assets/bg.png` 读取 cooked asset、policy bundle、scenario refs 和 player route model。
- TsuiNoSora private slice：`local:/probe/manifest.json` 读取合法本地源，cook 后写入 `package:/native-assets/...`；patch direct-read 只在授权 profile 下通过 scenario `mount_probes` 或 `mount_assets` 证明。
- FVP case：`.astrapkg` 保存 case profile、reader identity、release report 和 sanitized scenario；FVP `.bin` 通过 `fvp:/graph_bg/BG001_000`、`fvp:/voice/01000010` 这类 URI 进入 legacy VFS。
- AI ModelBundle：模型权重、runtime dependency 和 custom op sidecar 是 package-backed VFS entries，不走 project-level `package_sections` 携带 payload。

## Report Policy

VFS report 只写：

- `vfs_uri`、prefix、backend kind、profile 和 provider id。
- section id 或 pack/entry id、offset、size、hash、codec 和 media kind。
- overlay source hash、base hash、priority、allowlist id、change op 和 reason。
- diagnostic code、severity、reader identity/hash 和 redaction status。

Report 不写本地 root、绝对路径、payload bytes、商业文本、截图、音频、影片、bytecode、provider secret 或 native handle。`local_authorized` root 只能留在 host capability 内。

## Current Code Slice

当前实现已经把 package writer 从 `asset.registry` 迁到 `asset.vfs_manifest` + `asset.catalog`：

- `astra-asset` 提供 `VfsUri`、prefix/layer/entry/whiteout DTO、`AssetCatalog` 和 host-only `LocalMountRootSet`。
- `astra-package` 默认写入 `package` prefix、package layer、VFS entries 和 catalog rows，不再写 `asset.registry`。
- `astra-cli package build` 会把 cooked asset、project package section 和 NativeVN/TsuiNoSora asset sidecar 转成 package-backed VFS entry。
- `astra-plugin` 允许多个 `vfs_provider` 同 slot 共存。
- `astra-release` 校验 `vfs.uri_format`、`vfs.prefix_registry`、`vfs.package_mount`、`vfs.overlay_mount` 和 `vfs.catalog`，并阻断旧 registry、root leak、payload-like 字段、hash mismatch 和 bounds mismatch。

Stage 5 的 legacy pack reader 仍是计划项，本轮只保留 `legacy_pack` backend/prefix/diagnostic 入口。

## Tests

```bash
cargo test -p astra-asset vfs_uri
cargo test -p astra-asset vfs_overlayfs
cargo test -p astra-package package_vfs_mount
cargo test -p astra-plugin vfs_provider_registry
cargo test -p astra-release vfs_mount_gate
```

Expected report includes package mount pass、prefix/provider binding、catalog URI continuity、overlay whiteout policy and path/payload leak blocking diagnostics.

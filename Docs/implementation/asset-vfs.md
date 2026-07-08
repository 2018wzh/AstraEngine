# Asset VFS Blueprint

本页把 [Asset VFS Contract](../contracts/asset-vfs.md) 落到 workspace 边界。VFS 不是另一个 package 容器，也不是文件系统直通层；它是 `astra-asset` 拥有的统一读源、权限、hash 和证据模型。

## Ownership

| Crate | 职责 |
| --- | --- |
| `astra-asset` | `VfsLocator`、`VfsMountDescriptor`、mount family、relative key normalization、resolve report、redaction rule、schema |
| `astra-cook` | 把 import source、sidecar、native asset roots 和 ModelBundle source 解析成 VFS locator，并输出 cook audit |
| `astra-package` | 提供 package-backed mount，校验 section table、codec、offset、size 和 hash |
| provider plugin | 通过 `VfsMountProvider` 注册 legacy pack reader、authorized external source reader 或 overlay reader |
| `astra-release` | 校验 mount descriptor、reader identity/hash、overlay policy、payload redaction 和 package/source consistency |

## Mount Resolution

VFS resolve 的输入是 `(target, profile, alias, key, expected_hash, media_kind)`。Host 先按 target/profile 选出 mount set，再按固定 priority 查找：

1. `overlay` mount：只允许 profile 明确声明的 key pattern。
2. `package` mount：读取 `.astrapkg` section 或 patch package section。
3. `legacy_pack` mount：由 pack reader provider 解析 entry table。
4. `local_authorized` mount：只服务开发期、私有 acceptance 或用户显式授权 case。

命中结果必须携带 `mount_id`、source ref、entry ref、offset、size、hash、media kind 和 diagnostic。多 mount 命中且没有 overlay policy 时阻断。读取 bytes 前必须校验 bounds；读取后必须校验 hash policy。

## Package Relationship

`.astrapkg` 保留为控制面和证据面容器。它保存 target manifest、plugin registry、provider policy、compiled story、VN/AI/EMU package sections、release summary 和 cooked assets。旧引擎 pack 不替代 `.astrapkg`；它只作为 `legacy_pack` mount 给 AstraEMU 或迁移工具读取 payload。

典型组合：

- NativeVN release：`package` mount 读取 cooked asset、policy bundle、scenario refs 和 player route model。
- TsuiNoSora private slice：`local_authorized` mount 读取合法本地源，cook 后写 `package` mount；patch direct-read 只在授权 profile 下通过 `mount_probes` 或 `mount_assets` 证明。
- FVP case：`.astrapkg` 保存 case profile、reader identity、release report 和 sanitized scenario；FVP `.bin` 通过 `legacy_pack` mount 读取图像、音频、脚本或 movie entry。
- AI ModelBundle：模型权重、runtime dependency 和 custom op sidecar 是 package-backed VFS entries，不走 project-level `package_sections`。

## Report Policy

VFS report 只写：

- mount alias、mount kind、profile 和 provider id。
- relative key、pack id、entry id、offset、size、hash、codec 和 media kind。
- overlay source hash、base mount id、priority 和 allowlist id。
- diagnostic code、severity、reader identity/hash 和 redaction status。

Report 不写本地 root、绝对路径、payload bytes、商业文本、截图、音频、影片、bytecode、provider secret 或 native handle。`local_authorized` mount 的 root 只能留在 host capability 内，不进入 save、package 或 report。

## Gate Shape

Stage 2 的 VFS gate 在 `astra-release` 中执行：

- package mount：section table hash、schema registry、codec、bounds、hash 和 migration chain。
- local authorized mount：alias、relative key、profile、permission 和 redaction policy。
- legacy pack mount：reader capability、entry table hash、duplicate key、unsupported compression、offset/size bounds 和 media kind。
- overlay mount：priority、base mount、key allowlist、source hash 和 payload redaction。

Stage 3 NativeVN、Stage 4 AI/MCP 和 Stage 5 AstraEMU 只能复用这些 gate，不再各自实现私有路径校验。

## Tests

后续实现应补齐以下测试：

```bash
cargo test -p astra-asset vfs_mount_descriptor
cargo test -p astra-package package_vfs_mount
cargo test -p astra-cook vfs_locator_audit
cargo test -p astra-release vfs_mount_gate
astra test run scenarios/vfs/overlay_resolution.yaml --headless
```

Expected report includes package mount pass、authorized local mount redaction、legacy pack mount entry evidence、overlay priority decision and path/payload leak blocking diagnostics.

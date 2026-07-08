# Asset Pipeline Module

Asset Pipeline 从 text-first source、授权本地素材、package section 和 legacy pack entry 生成可发布资产。Importer 只写 source copy、sidecar 和 import audit；Cook processor 生成 cooked artifact；Package writer 组装自描述二进制容器。VFS 由 `astra-asset` 定义，负责 mount、locator、权限、hash 和证据。

## Crate 边界

| Crate | 职责 |
| --- | --- |
| `astra-asset` | AssetId、VFS mount contract、AssetRegistry、sidecar schema |
| `astra-cook` | Importer/CookProcessor traits、DDC key、VFS locator audit、package builder |
| `astra-package` | Binary container、section table、hash、Zstd codec、crypto descriptor、bounded read |

## Source Sidecar

```yaml
schema: astra.asset.v1
id: asset:/characters/hero/main
source: content/characters/hero/main.png
type: image.rgba
license: project-owned
importer: astra.import.image
cook:
  target: texture2d
  color_space: srgb
review: accepted
```

## Release Rules

Missing asset、invalid license、missing sidecar、stale cooked artifact、provider-ineligible artifact 和 schema migration gap 都是 blocking diagnostic。

VFS release rule 额外检查 mount family、relative key、reader identity、entry bounds、hash、overlay priority 和 redaction。Report 只记录 alias、relative key、pack/entry、offset、size、hash、media kind 和 diagnostic，不写本地 root 或 payload。

## 实现接口

`astra-asset` 暴露 `AssetId`、`AssetSidecar`、`AssetRegistry`、`VfsLocator`、`VfsMountDescriptor` 和 VFS resolve report。`astra-cook` 暴露 metadata importer、cook processor、`CookArtifact`、DDC key、VFS locator audit 和 import audit。`astra-package` 只接收 cooked artifact 和 manifest section，并作为 package-backed mount source；它不读取源素材目录，也不替代旧引擎 pack reader。

完整流程和默认检查见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md) 与 [Asset VFS Blueprint](../implementation/asset-vfs.md)。

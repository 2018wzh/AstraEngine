# Asset Pipeline Module

Asset Pipeline 从 text-first source 和外部素材生成 binary package。Importer 只写 source copy、sidecar 和 import audit；Cook processor 生成 cooked artifact；Package writer 组装自描述二进制容器。

## Crate 边界

| Crate | 职责 |
| --- | --- |
| `astra-asset` | AssetId、VFS、AssetRegistry、sidecar schema |
| `astra-cook` | Importer/CookProcessor traits、DDC key、package builder |
| `astra-package` | Binary container、section table、hash、streaming read |

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

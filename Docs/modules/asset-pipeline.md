# Asset Pipeline Module

Asset Pipeline 从 text-first source、授权本地素材、package section 和 legacy pack entry 生成可发布资产。Importer 只写 source copy、sidecar 和 import audit；Cook processor 生成 cooked artifact；Package writer 组装自描述二进制容器。VFS 由 `astra-asset` 定义，public locator 统一为 `provider:/path/file` 风格的 `VfsUri`，并负责 prefix、mount graph、权限、hash 和证据。

## Crate 边界

| Crate | 职责 |
| --- | --- |
| `astra-asset` | AssetId、`VfsUri`、prefix/layer/entry/whiteout DTO、`AssetCatalog`、sidecar schema |
| `astra-cook` | Importer/`CookProcessor` registry、依赖图、DDC key、持久内容缓存、bounded batch executor、取消和原子提交 |
| `astra-package` | Binary container、section table、`asset.vfs_manifest`、`asset.catalog`、hash、Zstd codec、crypto descriptor、bounded read |

## Source Sidecar

```yaml
schema: astra.asset.v1
id: asset:/characters/hero/main
source: content/characters/hero/main.png
source_hash: sha256:0000000000000000000000000000000000000000000000000000000000000000
type: image.rgba
license: project-owned
importer: astra.import.image
dependencies:
  - asset:/materials/character/default
cook:
  processor: astra.cook.texture2d
  target_profiles: [desktop-release]
  params:
    color_space: srgb
review: accepted
```

## Release Rules

Missing asset、invalid license、missing sidecar、stale cooked artifact、provider-ineligible artifact 和 schema migration gap 都是 blocking diagnostic。

VFS release rule 额外检查 `VfsUri`、prefix registry、provider binding、entry bounds、hash、overlay priority、whiteout allowlist 和 redaction。Report 只记录 `vfs_uri`、prefix、section or pack entry、offset、size、hash、media kind 和 diagnostic，不写本地 root 或 payload。`asset.registry` 不再是 package 内资产真源。

## 实现接口

`astra-asset` 暴露 `AssetId`、带 typed dependency 与 `FontAssetMetadata` 的 `AssetSidecar`、`VfsUri`、`VfsManifest`、`AssetCatalog`、`LocalMountRootSet` 和 VFS resolve report。字体 metadata把 family、face index、subset、Unicode coverage 与 sidecar source hash/license绑定，不能在 Player 端根据文件名猜测。`astra-cook` 暴露 metadata importer、`CookProcessorRegistry`、`CookBatchExecutor`、`CookCancellationToken`、`FileCookCache`、显式 `CookBatchLimits`、`CookArtifact`、DDC key 和 import audit。批次先验证重复/missing/self/cyclic dependency，再按拓扑层以受限并发执行；依赖 artifact hash 会进入下游 cache key，上游内容变化会确定性失效所有直接和传递依赖。cache corruption、processor/version/source/dependency identity drift、worker panic 和取消全部返回稳定 blocking error。CLI 把项目 cook 写到 sibling staging directory，完成后替换旧输出；失败或取消保留上一个完整输出。`.astra-cache/` 只保存 host-local 内容寻址 artifact，不进入 package/report。`astra.cook_manifest.v2` 只记录 graph hash、artifact/cache-hit/cooked count、font/locale contract identity 和并发上限，不写 payload 或本地 cache path。

`astra-package` 只接收 cooked artifact 和 manifest section，并作为 package-backed source；它不读取源素材目录，也不替代旧引擎 pack reader。

完整流程和默认检查见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md) 与 [Asset VFS Blueprint](../implementation/asset-vfs.md)。

# Asset Foundation

Status: Phase 3 implemented foundation.

## Overview

`AstraAsset` provides the foundation asset contract for source sidecars, VFS resolution, generated registry scans, dependency diagnostics, import preset descriptors, project template descriptors, AI draft metadata, review queue item descriptors, watch invalidation records, cook/package manifest DTOs, embedded package payload entries, and package reader integrity checks.

It does not implement production binary media import/cook transforms, a compressed binary package container, large-asset streaming IO, or hot reload rollback yet.

## Key Concepts

- `AssetUri` preserves source schemes such as `native:/`, `virtual:/`, `package:/`, and `foreign-*:/`.
- `AssetUri::ToStableId()` maps asset references into the Core `AssetId` kind for cross-system compatibility.
- Source sidecars are YAML files ending in `.asset.yaml`.
- Registry data is generated from sidecars and should not be edited by humans, AI, or MCP tools.
- Current NativeVN packages embed cooked payloads in `astra.package.manifest.v1` as base64 payload entries with SHA-256 hash and size metadata.
- `PackageReader` validates package, cook manifest, DDC metadata, payload encoding, payload hash, and payload size before returning bytes, text, chunked reads, or a read-only package mount DTO.
- AI-generated or review-required assets must have accepted review before entering the foundation registry.

## Architecture

Design references:

- [Asset Pipeline](../../../design/asset-pipeline.md)
- [Content and Assets](../../../design/content-and-assets.md)
- [Implementation Coverage](../../../design/implementation-coverage.md)

`AstraAsset` depends on Core and Platform. It does not depend on Runtime, Media, Script, Editor, AI provider implementations, or legacy compatibility modules.

## Programming Guide

Use `ParseAssetUri()` for user/source references and keep the original URI scheme when showing diagnostics. Use `Vfs::Mount()` and `Vfs::Resolve()` for foundation path resolution. Use `AssetRegistryBuilder::Scan()` to read `.asset.yaml` files and emit diagnostics for duplicate IDs, missing source files, and broken hard dependencies.

Use descriptor validators for project templates, import presets, and review queue items as schema-level checks only. They are not production import/cook workflows.

Use `PackageReader::ReadManifest()` before trusting a package report. Use `ReadPayloadBytes()` or `ReadPayloadText()` for random access to a cooked asset payload, `ReadPayloadChunks()` when testing streaming-read behavior, and `MountPackage()` to produce a read-only package mount summary for runtime launch evidence.

## API Reference

Implemented header:

- `Engine/Runtime/Asset/Public/Astra/Asset/Asset.hpp`

Primary DTOs:

- `AssetUri`
- `VfsMount`
- `AssetSidecar`
- `AssetRegistryEntry`
- `ImportPresetDescriptor`
- `ProjectTemplateDescriptor`
- `ReviewQueueItem`
- `CookManifest`
- `DerivedDataCacheEntry`
- `PackagePayloadEntry`
- `PackagePayloadChunk`
- `PackageManifest`
- `PackageMount`
- `PackageReader`

## Examples

```yaml
id: native:/Characters/Alice/Normal
type: image
source_path: alice.png
display_name: Alice Normal
origin: HumanAuthored
license:
  owner: project
  usage: internal
review:
  status: accepted
cook:
  preset: sprite
dependencies:
  hard: []
  soft: []
```

## Troubleshooting

- `ASTRA_ASSET_ID_DUPLICATE` means two sidecars claim the same asset URI.
- `ASTRA_ASSET_DEPENDENCY_MISSING` means a hard dependency is not present in the generated registry.
- `ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH` means an embedded package payload no longer matches its manifest or cook artifact hash.
- `ASTRA_PACKAGE_PAYLOAD_MISSING` means a cook artifact was listed without an embedded package payload.
- `ASTRA_RELEASE_ASSET_004` means an AI-generated or review-required asset is not accepted.

# Asset Pipeline

Status: Phase 6 implemented production Asset Pipeline slice.

## Overview

`AstraAsset` provides the asset contract for source sidecars, VFS resolution, generated registry scans, dependency diagnostics, importer descriptors, cook processors, local DDC read/write/rebuild/clean reports, binary `.astrapkg` writing/reading, package mount policy, hot reload rollback DTOs, and Asset Release Gate checks.

It does not implement Phase 7 media execution: real renderer upload, executable font atlas/glyph rendering, mixer playback, or GPU filter execution.

## Key Concepts

- `AssetUri` preserves source schemes such as `native:/`, `virtual:/`, `package:/`, and `foreign-*:/`.
- `AssetUri::ToStableId()` maps asset references into the Core `AssetId` kind for cross-system compatibility.
- Source sidecars are YAML files ending in `.asset.yaml`.
- Registry data is generated from sidecars and should not be edited by humans, AI, or MCP tools.
- `ImportRequest` and `IAssetImporter` write accepted source copies and sidecars for image, audio, font, text, filter profile and script-like sources.
- `CookAssetRegistry()` writes deterministic cook artifacts and local DDC entries; corrupt DDC payloads are rebuilt rather than trusted, and `CleanDerivedDataCache()` removes stale cache artifacts while retaining live entries.
- Current packages are binary `.astrapkg` files with `ASTRAP6\0` header, embedded canonical JSON manifest, zstd-compressed payloads, offsets, compressed sizes and SHA-256 payload hashes.
- `PackageReader` validates package header, manifest, cook manifest, DDC metadata, payload encoding/compression, payload hash, and payload size before returning bytes, text, chunked reads, or a read-only package mount DTO.
- AI-generated or review-required assets must have accepted review before entering the foundation registry.
- Save/replay evidence records package manifest hash, package profile and provider feature hash when running from package.

## Architecture

Design references:

- [Asset Pipeline](../../../design/asset-pipeline.md)
- [Content and Assets](../../../design/content-and-assets.md)
- [Implementation Coverage](../../../design/implementation-coverage.md)

`AstraAsset` depends on Core and Platform. It does not depend on Runtime, Media, Script, Editor, AI provider implementations, or legacy compatibility modules.

## Programming Guide

Use `ParseAssetUri()` for user/source references and keep the original URI scheme when showing diagnostics. Use `Vfs::Mount()` and `Vfs::Resolve()` for foundation path resolution. Use `AssetRegistryBuilder::Scan()` to read `.asset.yaml` files and emit diagnostics for duplicate IDs, missing source files, and broken hard dependencies.

Use descriptor validators for project templates, import presets, importer descriptors and review queue items as schema-level checks. Use `CreateBuiltinImporter()`/`IAssetImporter` for import, and use `CookAssetRegistry()` for production cook/DDC behavior instead of duplicating package logic in tools.

Use `PackageWriter::WritePackage()` to emit binary `.astrapkg` containers. Use `PackageReader::ReadManifest()` before trusting a package report. Use `ReadPayloadBytes()` or `ReadPayloadText()` for random access to a cooked asset payload, `ReadPayloadChunks()` when testing streaming-read behavior, and `MountPackage()` to produce a read-only package mount summary for runtime launch evidence.

## API Reference

Implemented header:

- `Engine/Runtime/Asset/Public/Astra/Asset/Asset.hpp`

Primary DTOs:

- `AssetUri`
- `VfsMount`
- `AssetSidecar`
- `AssetRegistryEntry`
- `ImportPresetDescriptor`
- `ImportRequest`
- `ImporterDescriptor`
- `IAssetImporter`
- `ProjectTemplateDescriptor`
- `ReviewQueueItem`
- `DdcKey`
- `DdcCleanReport`
- `CookRequest`
- `CookArtifactDescriptor`
- `ICookProcessor`
- `CookManifest`
- `DerivedDataCacheEntry`
- `PackageWriter`
- `PackagePayloadEntry`
- `PackagePayloadRef`
- `PackagePayloadChunk`
- `PackageManifest`
- `PackageMountPolicy`
- `PackageMount`
- `PackageReader`
- `AssetReleaseGateReport`
- `HotReloadTransaction`

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
- `ASTRA_PACKAGE_HEADER_INVALID` means the `.astrapkg` binary header is corrupt or unsupported.
- `ASTRA_RELEASE_DDC_HASH_MISMATCH` means cook artifact metadata and DDC metadata disagree.
- `ASTRA_RELEASE_ASSET_004` means an AI-generated or review-required asset is not accepted.

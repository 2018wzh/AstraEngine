# Asset Foundation

Status: Phase 3 implemented foundation.

## Overview

`Astra_Asset` provides the foundation asset contract for source sidecars, VFS resolution, generated registry scans, dependency diagnostics, import preset descriptors, project template descriptors, AI draft metadata, review queue item descriptors, and watch invalidation records.

It does not implement production import, cook, DerivedDataCache, package writing, package reading, or hot reload rollback yet.

## Key Concepts

- `AssetUri` preserves source schemes such as `native:/`, `virtual:/`, `package:/`, and `foreign-*:/`.
- `AssetUri::ToStableId()` maps asset references into the Core `AssetId` kind for cross-system compatibility.
- Source sidecars are YAML files ending in `.asset.yaml`.
- Registry data is generated from sidecars and should not be edited by humans, AI, or MCP tools.
- AI-generated or review-required assets must have accepted review before entering the foundation registry.

## Architecture

Design references:

- [Asset Pipeline](../../../design/asset-pipeline.md)
- [Content and Assets](../../../design/content-and-assets.md)
- [Implementation Coverage](../../../design/implementation-coverage.md)

`Astra_Asset` depends on Core and Platform. It does not depend on Runtime, Media, Script, Editor, AI provider implementations, or legacy compatibility modules.

## Programming Guide

Use `ParseAssetUri()` for user/source references and keep the original URI scheme when showing diagnostics. Use `Vfs::Mount()` and `Vfs::Resolve()` for foundation path resolution. Use `AssetRegistryBuilder::Scan()` to read `.asset.yaml` files and emit diagnostics for duplicate IDs, missing source files, and broken hard dependencies.

Use descriptor validators for project templates, import presets, and review queue items as schema-level checks only. They are not production import/cook workflows.

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
- `ASTRA_RELEASE_ASSET_004` means an AI-generated or review-required asset is not accepted.

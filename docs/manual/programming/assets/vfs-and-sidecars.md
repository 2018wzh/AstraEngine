# VFS And Sidecars

Status: Phase 6 implemented asset/package slice.

## Overview

The VFS maps asset URI schemes to mounted directories and reports read-only policy. Sidecars describe source assets in YAML so CLI, tests, future Editor tools, cook/package, save/replay evidence, and release gates can share one metadata contract.

## Key Concepts

- Mount priority chooses the first matching scheme.
- `native:/` is project-owned source content.
- `foreign-*:/` is read-only by default for compatibility or external mounts.
- `virtual:/` references are runtime/editor resolver references and are not source sidecar IDs.
- `package:/` is represented by read-only `PackageMount` DTOs produced from binary `.astrapkg` manifests.

## Architecture

See [Asset Pipeline](../../../design/asset-pipeline.md) and [Content and Assets](../../../design/content-and-assets.md).

## Programming Guide

Create a `VfsMount` with `mount_id`, `scheme`, `root`, `access`, and `priority`, then call `Vfs::Resolve()` with an `AssetUri`. The returned `ResolvedAssetPath` includes the chosen mount and read-only flag.

Sidecars should include `id`, `type`, `source_path`, `license`, `review`, and optional dependency lists. Registry scans emit blocking diagnostics for missing source files, duplicate IDs, and missing hard dependencies.

Package mounts are read-only by policy. Use `PackageReader::MountPackage()` to inspect package identity, assets, random-access support and chunked-read support; use `ReadPayloadBytes()`/`ReadPayloadChunks()` for current runtime package evidence.

## API Reference

- `Vfs`
- `VfsMount`
- `ResolvedAssetPath`
- `AssetSidecar`
- `AssetRegistryBuilder`

## Examples

```yaml
mount_id: project.content
scheme: native
root: Content
access: read_write
priority: 100
```

## Troubleshooting

- Use `native:/` only for project-owned assets.
- Do not put generated registry files under manual editing workflows.
- Package mounts do not allow writes or foreign copies by default; corrupt package headers or payload hashes are blocking diagnostics.

# FilterGraph Foundation

Status: Phase 3 implemented foundation.

## Overview

Phase 3 implements `FilterProfile` validation and deterministic headless application records for layer-aware 2D/VN effects. GPU filter execution is future production media work.

## Key Concepts

- Valid targets are `background`, `character`, `ui`, `text`, and `final`.
- Each pass must have an `id` and `filter`.
- Pass parameters are captured through deterministic JSON hashes.
- Filter profiles are source assets and should use `native:/Filters/...` IDs.

## Architecture

See [Media Runtime](../../../design/media-runtime.md) and [Content and Assets](../../../design/content-and-assets.md).

## Programming Guide

Use `FilterProfileFromJson()` to parse source data and `ValidateFilterProfile()` before applying it to a headless render graph. `ApplyFilterProfile()` returns application records that become part of the frame capture hash.

## API Reference

- `FilterProfile`
- `FilterPass`
- `FilterTarget`
- `FilterProfileFromJson()`
- `ValidateFilterProfile()`
- `ApplyFilterProfile()`

## Examples

```yaml
id: native:/Filters/soft_vn
passes:
  - id: bg_blur
    filter: astra.filter.gaussian_blur
    target: background
    params:
      radius: 2
```

## Troubleshooting

- Invalid targets are blocking because layer names must be shared by tools, runtime, headless verification, and future release gates.
- Phase 3 release-gate checks validate selected media providers and headless fallback support. Real filter implementation/provider execution checks remain future production media work.

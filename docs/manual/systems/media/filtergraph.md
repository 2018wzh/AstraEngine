# FilterGraph

Status: Phase 7 provider evidence implemented.

## Overview

`FilterProfile` validation now feeds Phase 7 FilterGraph provider evidence. The production provider descriptor is `astra.filter_graph.bgfx`; CI and deterministic runs use `ExecuteFilterGraphHeadless()` to record the same layer-aware pass order and output hash without exposing backend handles.

## Key Concepts

- Valid targets are `background`, `character`, `ui`, `text`, and `final`.
- Each pass must have an `id` and `filter`.
- Pass parameters are captured through deterministic JSON hashes.
- Filter profiles are source assets and should use `native:/Filters/...` IDs.
- Built-in pass names used by current evidence include gaussian blur, line enhance, color grade, and pass-through.

## Architecture

See [Media Runtime](../../../design/media-runtime.md) and [Content and Assets](../../../design/content-and-assets.md).

## Programming Guide

Use `FilterProfileFromJson()` to parse source data and `ValidateFilterProfile()` before applying it to a render graph. `ApplyFilterProfile()` returns application records, and `ExecuteFilterGraphHeadless()` emits provider execution evidence and a deterministic output hash.

## API Reference

- `FilterProfile`
- `FilterPass`
- `FilterTarget`
- `FilterProfileFromJson()`
- `ValidateFilterProfile()`
- `ApplyFilterProfile()`
- `ExecuteFilterGraphHeadless()`
- `FilterGraphExecution`

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
- Phase 7 release-gate checks validate selected filter provider descriptors and headless fallback support.
- Unsupported filters in deterministic release should block unless the profile explicitly permits fallback.



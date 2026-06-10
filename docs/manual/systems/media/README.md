# Media Foundation

Status: Phase 3 implemented foundation.

## Overview

`AstraMedia` defines the foundation presentation and media DTOs used by future Script, AstraVN, Renderer2D, TextLayout, Audio, Timeline, and FilterGraph systems. Phase 3 verifies command extraction, provider descriptor selection, mature backend capability probing, image metadata inspection, image cook artifact metadata, foundation release-gate checks, and deterministic headless hashes; it does not yet upload decoded textures, execute shaped glyph runs, play audio, execute GPU filters, or run production timelines.

## Key Concepts

- `PresentationCommand` records logical sprite, text, UI, audio, filter, and timeline-style requests.
- `RenderGraph` groups draw, text, audio, and filter application records.
- `HeadlessRenderer2D` captures deterministic render/text/audio/filter hashes for CI and release evidence.
- `FilterProfile` targets `background`, `character`, `ui`, `text`, and `final`.
- `MediaProviderDescriptor` declares the required Phase 3 slots: `astra.renderer2d`, `astra.text_layout`, and `astra.audio`.
- `MediaBackendCapabilityReport` records available mature backend libraries and feature readiness for image decode, text/font shaping, and audio mixing.
- `ImageDecodeReport` records PNG/JPEG/WebP metadata decoded through mature libraries without exposing backend handles.
- `ValidateMediaReleaseGate()` checks selected foundation providers for slot match, packaged eligibility, diagnostics prefix, supported formats/features, and headless fallback support.
- SDL support is currently a private compile-path factory stub that falls back to headless behavior; libpng, libjpeg-turbo, libwebp, FreeType, HarfBuzz, and miniaudio are probed as backend capability evidence.

## Architecture

Design references:

- [Media Runtime](../../../design/media-runtime.md)
- [Runtime Core](../../../design/runtime-core.md)
- [Samples and Test Matrix](../../../design/samples-and-test-matrix.md)

`AstraMedia` depends on Core and Asset. It does not expose SDL, GPU handles, audio device handles, font internals, Editor widgets, or script/VN-specific runtime objects.

## Programming Guide

Build a vector of `PresentationCommand` DTOs, optionally parse or construct a `FilterProfile`, and call `ExtractRenderGraph()`. Submit the graph to `CreateHeadlessRenderer2D()` and inspect `FrameCapture` hashes.

Use `ProbeMediaBackendCapabilities()` when a foundation sample or release report needs to prove which mature libraries are available. Use `InspectImageBytes()` to validate PNG/JPEG/WebP payload metadata before future texture upload/cook steps. `astra cook` records decoded image metadata under cook artifact `metadata.media_inspect` for supported image sources, and records skipped metadata for non-decodable placeholders. Use `FoundationMediaProviders()` and `ValidateMediaReleaseGate()` when a foundation sample needs release evidence for renderer/text/audio provider selection. The foundation release gate validates contracts, detected formats/features, and deterministic fallback support; it does not prove real media output.

Use this as foundation verification only. Production providers will later implement real renderer, text layout, audio, timeline, and filter execution behind the same boundary.

## API Reference

Implemented header:

- `Engine/Runtime/Media/Public/Astra/Media/Media.hpp`

Primary DTOs:

- `PresentationCommand`
- `RenderGraph`
- `TextLayoutRequest`
- `AudioCommand`
- `FilterProfile`
- `FrameCapture`
- `MediaProviderDescriptor`
- `MediaReleaseGateRequest`
- `MediaReleaseGateReport`
- `MediaBackendLibrary`
- `MediaBackendCapabilityReport`
- `ImageDecodeReport`

Primary helpers:

- `FoundationMediaProviders()`
- `ProbeMediaBackendCapabilities()`
- `InspectImageBytes()`
- `ValidateMediaProviderDescriptor()`
- `ValidateMediaReleaseGate()`

## Examples

The CLI `astra validate Samples/NativeVN --strict --json` emits `phase3_media_backend_capabilities` and `phase3_media_release_gate`. `astra run Samples/PackageSmoke --headless-smoke --json` emits the same media capability/release-gate evidence, a libpng `image_decode_smoke`, and stable headless render/text/audio/filter hashes.

## Troubleshooting

- `ASTRA_MEDIA_FILTER_TARGET_INVALID` means a filter pass target is not one of the five foundation layers.
- `ASTRA_MEDIA_LAYER_UNKNOWN` means a draw command references a layer missing from the render graph.
- `ASTRA_MEDIA_RELEASE_*` diagnostics mean the selected foundation provider set cannot pass packaged/headless release-gate checks.
- Real playback/rendering failures are not represented yet because production media execution backends are future Phase 7 work.

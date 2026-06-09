# Media Foundation

Status: Phase 3 implemented foundation.

## Overview

`Astra_Media` defines the foundation presentation and media DTOs used by future Script, AstraVN, Renderer2D, TextLayout, Audio, Timeline, and FilterGraph systems. Phase 3 verifies command extraction, provider descriptor selection, foundation release-gate checks, and deterministic headless hashes; it does not decode images, shape fonts, play audio, execute GPU filters, or run production timelines.

## Key Concepts

- `PresentationCommand` records logical sprite, text, UI, audio, filter, and timeline-style requests.
- `RenderGraph` groups draw, text, audio, and filter application records.
- `HeadlessRenderer2D` captures deterministic render/text/audio/filter hashes for CI and release evidence.
- `FilterProfile` targets `background`, `character`, `ui`, `text`, and `final`.
- `MediaProviderDescriptor` declares the required Phase 3 slots: `astra.renderer2d`, `astra.text_layout`, and `astra.audio`.
- `ValidateMediaReleaseGate()` checks selected foundation providers for slot match, packaged eligibility, diagnostics prefix, supported formats/features, and headless fallback support.
- SDL support is currently a private compile-path factory stub that falls back to headless behavior.

## Architecture

Design references:

- [Media Runtime](../../../design/media-runtime.md)
- [Runtime Core](../../../design/runtime-core.md)
- [Samples and Test Matrix](../../../design/samples-and-test-matrix.md)

`Astra_Media` depends on Core and Asset. It does not expose SDL, GPU handles, audio device handles, font internals, Editor widgets, or script/VN-specific runtime objects.

## Programming Guide

Build a vector of `PresentationCommand` DTOs, optionally parse or construct a `FilterProfile`, and call `ExtractRenderGraph()`. Submit the graph to `CreateHeadlessRenderer2D()` and inspect `FrameCapture` hashes.

Use `FoundationMediaProviders()` and `ValidateMediaReleaseGate()` when a foundation sample needs release evidence for renderer/text/audio provider selection. The foundation release gate validates contracts and deterministic fallback support; it does not prove real media output.

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

Primary helpers:

- `FoundationMediaProviders()`
- `ValidateMediaProviderDescriptor()`
- `ValidateMediaReleaseGate()`

## Examples

The CLI `astra validate Samples/NativeVN --strict --json` emits `phase3_media_release_gate`. `astra run Samples/PackageSmoke --headless-smoke --json` emits the same media release-gate evidence plus stable headless render/text/audio/filter hashes.

## Troubleshooting

- `ASTRA_MEDIA_FILTER_TARGET_INVALID` means a filter pass target is not one of the five foundation layers.
- `ASTRA_MEDIA_LAYER_UNKNOWN` means a draw command references a layer missing from the render graph.
- `ASTRA_MEDIA_RELEASE_*` diagnostics mean the selected foundation provider set cannot pass packaged/headless release-gate checks.
- Real playback/rendering failures are not represented yet because production media backends are future Phase 7 work.

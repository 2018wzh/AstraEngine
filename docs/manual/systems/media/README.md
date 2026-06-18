# Media Runtime

Status: Phase 7 backend evidence implemented; optional bgfx/Skia production providers are hardened behind private boundaries.

## Overview

`AstraMedia` defines presentation and media DTOs plus Phase 7 provider evidence for Renderer2D, TextLayout, Audio, Image/Audio/Video decode slots, Timeline, and FilterGraph. Current evidence covers command extraction, production provider selection, package/source image decode, audio metadata decode, texture-buffer import, glyph-run/atlas capture, audio bus logical state, timeline camera/audio/filter state, FilterGraph execution records, video decode extension-point diagnostics, deterministic headless hashes, and opt-in bgfx/Skia provider smoke diagnostics.

## Key Concepts

- `PresentationCommand` records logical sprite, text, UI, audio, filter, and timeline-style requests.
- `RenderGraph` groups draw, text, audio, and filter application records.
- `HeadlessRenderer2D` captures deterministic render/text/audio/filter hashes for CI and release evidence.
- `FilterProfile` targets `background`, `character`, `ui`, `text`, and `final`.
- `MediaProviderDescriptor` declares foundation slots plus Phase 7 slots: `astra.image_decode`, `astra.audio_decode`, `astra.video_decode`, `astra.timeline`, and `astra.filter_graph`.
- `MediaBackendCapabilityReport` records available mature backend libraries and feature readiness for image decode, text/font shaping, audio mixing, bgfx Renderer2D, and Skia UI/text raster.
- `ImageDecodeReport` records PNG/JPEG/WebP metadata decoded through mature libraries without exposing backend handles.
- `ValidateMediaReleaseGate()` checks selected foundation providers for slot match, packaged eligibility, diagnostics prefix, supported formats/features, and headless fallback support.
- SDL3, bgfx, Skia, libpng, libjpeg-turbo, libwebp, FreeType, HarfBuzz, miniaudio, and FFmpeg are kept behind private implementation boundaries. Public API exposes only DTOs and opaque tokens.

## Architecture

Design references:

- [Media Runtime](../../../design/media-runtime.md)
- [Runtime Core](../../../design/runtime-core.md)
- [Samples and Test Matrix](../../../design/samples-and-test-matrix.md)

`AstraMedia` depends on Core and Asset. It does not expose SDL, GPU handles, audio device handles, font internals, Editor widgets, or script/VN-specific runtime objects.

## Programming Guide

Build a vector of `PresentationCommand` DTOs, optionally parse or construct a `FilterProfile` or `TimelineAsset`, and call `ExtractRenderGraph()`. Submit the graph to `CreateHeadlessRenderer2D()` for foundation hashes or `CreateHeadlessRenderer2DProvider()` for the Phase 7 provider contract.

Use `ProductionMediaProviders()` and `ValidateMediaReleaseGate()` for Phase 7 release evidence. Set `require_available_backends` for opt-in bgfx/Skia checks such as `astra run ... --windowed-smoke --gpu-smoke --json`. Use `DecodeImageCpuBufferBytes()`, text/audio providers, `ExecuteFilterGraphHeadless()`, and `EvaluateTimeline()` to produce package-safe media execution evidence without exposing native handles.

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
- `DecodedCpuBuffer`
- `GlyphRun`
- `AudioStateCapture`
- `TimelineAsset`
- `TimelineState`
- `FilterGraphExecution`

Primary helpers:

- `FoundationMediaProviders()`
- `ProductionMediaProviders()`
- `ProbeMediaBackendCapabilities()`
- `InspectImageBytes()`
- `DecodeImageCpuBufferBytes()`
- `CreateHeadlessRenderer2DProvider()`
- `CreateProductionRenderer2DProvider()`
- `CreateFoundationTextLayoutProvider()`
- `CreateProductionTextLayoutProvider()`
- `CreateFoundationAudioProvider()`
- `TimelineFromJson()`
- `EvaluateTimeline()`
- `ExecuteFilterGraphHeadless()`
- `ValidateMediaProviderDescriptor()`
- `ValidateMediaReleaseGate()`

## Examples

The CLI `astra validate Samples/NativeVN --strict --json` emits `phase3_media_backend_capabilities`, `phase3_media_release_gate`, and `phase7_media_backend`. `astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --json` includes `playable_vn.phase7_media_execution` with renderer/text/audio/filter/timeline provider evidence.

## Troubleshooting

- `ASTRA_MEDIA_FILTER_TARGET_INVALID` means a filter pass target is not one of the five foundation layers.
- `ASTRA_MEDIA_LAYER_UNKNOWN` means a draw command references a layer missing from the render graph.
- `ASTRA_MEDIA_RELEASE_*` diagnostics mean the selected foundation provider set cannot pass packaged/headless release-gate checks.
- Cross-driver pixel diff, native playback verification, and video frame decode remain later hardening work; current Phase 7 evidence uses provider descriptors, opt-in bgfx/Skia availability checks, extension-point diagnostics, and deterministic headless fallback hashes.

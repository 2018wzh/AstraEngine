# Media Runtime

Status: Runtime-only production media backend evidence with driver diff hardening.

## Overview

`AstraMedia` defines presentation and media DTOs plus production provider evidence for Renderer2D, TextLayout, Audio, Image/Audio/Video decode slots, Timeline, and FilterGraph. Current evidence covers command extraction, production provider selection, package/source image decode, audio metadata decode, texture-buffer import, glyph-run/atlas capture, audio bus logical state, timeline camera/audio/filter state, CPU RGBA FilterGraph execution, deterministic headless hashes, bgfx/Skia/miniaudio capability checks, and driver diff reports.

## Key Concepts

- `PresentationCommand` records logical sprite, text, UI, audio, filter, and timeline-style requests.
- `RenderGraph` groups draw, text, audio, and filter application records.
- `HeadlessRenderer2D` captures deterministic render/text/audio/filter hashes for CI and release evidence.
- `FilterProfile` targets `background`, `character`, `ui`, `text`, and `final`.
- `MediaProviderDescriptor` declares foundation slots plus Phase 7 slots: `astra.image_decode`, `astra.audio_decode`, `astra.video_decode`, `astra.timeline`, and `astra.filter_graph`.
- `MediaBackendCapabilityReport` records available mature backend libraries and feature readiness for image decode, text/font shaping, audio mixing, bgfx Renderer2D, and Skia UI/text raster.
- `DriverDiffReport` compares headless/reference capture against production provider capture and records missing hardening capabilities.
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

Use `ProductionMediaProviders()` and `ValidateMediaReleaseGate()` for runtime release evidence. Set `require_available_backends` for deterministic bgfx/Skia/miniaudio checks. Use `DecodeImageCpuBufferBytes()`, text/audio providers, `ExecuteFilterGraphCpu()`, `CompareDriverCaptures()`, and `EvaluateTimeline()` to produce package-safe media execution evidence without exposing native handles.

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
- `FrameCaptureRequest`
- `AudioCaptureRequest`
- `DriverDiffReport`
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
- `ExecuteFilterGraphCpu()`
- `CompareDriverCaptures()`
- `ValidateMediaProviderDescriptor()`
- `ValidateMediaReleaseGate()`

## Examples

The CLI `astra validate Samples/NativeVN --strict --json` emits media backend capabilities and provider evidence. `astra package Samples/NativeVN --profile deterministic --json` embeds the runtime release report. `astra release-gate Samples/NativeVN --profile deterministic --json` emits `driver_diff`, media release gate, trace events, and crash bundle evidence.

## Troubleshooting

- `ASTRA_MEDIA_FILTER_TARGET_INVALID` means a filter pass target is not one of the five foundation layers.
- `ASTRA_MEDIA_LAYER_UNKNOWN` means a draw command references a layer missing from the render graph.
- `ASTRA_MEDIA_RELEASE_*` diagnostics mean the selected foundation provider set cannot pass packaged/headless release-gate checks.
- Video frame decode remains an extension point. Runtime-only driver diff hardening is implemented as capture/hash evidence; Editor visual diff viewers remain future tooling.

# Headless Renderer

Status: Phase 3 implemented foundation.

## Overview

The headless renderer records render, text, audio, and filter commands and produces deterministic hashes. It is intended for tests, CLI smoke reports, Phase 3 media release-gate foundation evidence, and future production release gates.

## Key Concepts

- Draw commands are sorted by layer and order before capture.
- Text and audio are logical command hashes, not font glyph or waveform output.
- Filter applications are layer-aware records, not GPU shader execution.
- Frame captures are JSON-serializable.
- Foundation provider release-gate checks require a headless fallback path so CI can verify media command determinism without a real device.

## Architecture

See [Media Runtime](../../../design/media-runtime.md).

## Programming Guide

Call `CreateHeadlessRenderer2D()`, submit a `RenderGraph`, then call `Capture()`. Re-submitting the same graph must produce the same hashes.

## API Reference

- `IRenderer2D`
- `CreateHeadlessRenderer2D()`
- `FrameCapture`
- `ValidateMediaReleaseGate()`
- `ToJson(const FrameCapture&)`

## Examples

```text
PresentationCommand -> ExtractRenderGraph -> HeadlessRenderer2D -> FrameCapture hashes
```

## Troubleshooting

- Empty hashes usually mean the corresponding command category is empty.
- Unknown layer diagnostics are blocking because render order must be deterministic.

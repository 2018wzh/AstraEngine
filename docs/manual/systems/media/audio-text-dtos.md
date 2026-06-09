# Audio And Text DTOs

Status: Phase 3 implemented foundation.

## Overview

Phase 3 represents text and audio as logical DTOs that can be hashed, inspected, saved later, and routed to future production providers. It does not rasterize fonts, shape glyphs, decode audio, mix buses, or play sound.

## Key Concepts

- `TextLayoutRequest` stores text, locale, target layer, order, and style metadata.
- `AudioCommand` stores logical play commands, asset URI, bus, volume, and loop state.
- Headless captures hash DTOs so tests can verify deterministic command order.
- `MediaProviderDescriptor` records the foundation text/audio provider contracts used by release-gate evidence.
- Production text/font and audio providers will consume these DTOs later.

## Architecture

See [Media Runtime](../../../design/media-runtime.md).

## Programming Guide

Create `PresentationCommandKind::Text` or `PresentationCommandKind::Audio` commands and pass them through `ExtractRenderGraph()`. The resulting `RenderGraph` contains `text_requests` and `audio_commands`, which `HeadlessRenderer2D` includes in `FrameCapture` hashes.

## API Reference

- `PresentationCommand`
- `TextLayoutRequest`
- `AudioCommand`
- `RenderGraph`
- `FrameCapture`
- `MediaProviderDescriptor`
- `ValidateMediaProviderDescriptor()`

## Examples

```text
Dialogue command -> TextLayoutRequest -> text_hash
Voice command -> AudioCommand -> audio_hash
```

## Troubleshooting

- Text hashes prove logical command determinism, not glyph output.
- Audio hashes prove logical command determinism, not playback output.
- Missing font and audio decode diagnostics are future production media backend work.

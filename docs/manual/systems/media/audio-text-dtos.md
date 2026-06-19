# Audio And Text DTOs

Status: Phase 7 provider evidence implemented.

## Overview

Text and audio still enter Media as logical DTOs, but Phase 7 adds provider execution evidence. FreeType/HarfBuzz shape glyph runs and atlas tokens through the text provider, while miniaudio-backed audio provider state records voice/music/sfx/ui/ambient bus routing with silent/headless fallback for CI.

## Key Concepts

- `TextLayoutRequest` stores text, locale, target layer, order, and style metadata.
- `AudioCommand` stores logical play commands, asset URI, bus, volume, and loop state.
- Headless captures hash DTOs so tests can verify deterministic command order.
- `MediaProviderDescriptor` records `astra.text_layout.skia_ui` and `astra.audio.miniaudio` for release-gate evidence; the current text raster path still uses FreeType/HarfBuzz internally for deterministic glyph evidence.
- `TextLayoutCapture` and `AudioStateCapture` provide deterministic hashes for replay and CI.
- Save/replay stores logical text/audio state, not glyph atlas tokens or native audio handles.

## Architecture

See [Media Runtime](../../../design/media-runtime.md).

## Programming Guide

Create `PresentationCommandKind::Text` or `PresentationCommandKind::Audio` commands and pass them through `ExtractRenderGraph()`. Use `CreateFoundationTextLayoutProvider()` to shape and capture glyph runs, and `CreateFoundationAudioProvider()` to submit audio commands and capture logical mixer state.

## API Reference

- `PresentationCommand`
- `TextLayoutRequest`
- `AudioCommand`
- `RenderGraph`
- `FrameCapture`
- `MediaProviderDescriptor`
- `MediaBackendCapabilityReport`
- `GlyphRun`
- `TextLayoutCapture`
- `AudioStateCapture`
- `ProbeMediaBackendCapabilities()`
- `ValidateMediaProviderDescriptor()`
- `CreateFoundationTextLayoutProvider()`
- `CreateFoundationAudioProvider()`

## Examples

```text
Dialogue command -> TextLayoutRequest -> GlyphRun -> TextLayoutCapture
Voice command -> AudioCommand -> AudioStateCapture
```

## Troubleshooting

- Text capture proves glyph-run determinism and atlas-token preparation, not saved native atlas state.
- Audio capture proves bus routing and logical playback state; CI may use silent backend fallback.
- Native device failures should emit `ASTRA_AUDIO_*` diagnostics and fall back only when the selected profile allows it.



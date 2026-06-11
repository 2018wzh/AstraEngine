# Platform Backend Porting

Status: Phase 1 implemented foundation.

## Overview

Platform exposes public services for window, filesystem, dynamic library, thread, timer, and crash capture. Phase 1 includes a headless backend, SDL-backed window backend compile path, opaque dynamic library handles, watch polling, pending task tags, and crash capture context.

## Key Concepts

- Public headers do not expose SDL or OS/native handles.
- Headless backend is the CI and smoke-test default.
- SDL is confined to private implementation files.
- Dynamic libraries are represented publicly by `DynamicLibraryHandle`; native handles remain private.
- Crash capture can include build info, frame index, thread id, recent logs, and package/project hash.

## Architecture

See [Platform Services](../../../design/foundation-core-platform-property.md).

## Programming Guide

Implement the public service interfaces in `Astra/Platform/Platform.hpp`. Backend factories return `PlatformServices`; failures report diagnostics or `Result` errors rather than crashing. Use `PollWatches()` for headless watch invalidation tests and `PendingTags()`/`Drain()` to prove thread queue shutdown behavior.

## API Reference

Use `Engine/Runtime/Platform/Public/Astra/Platform/Platform.hpp`.

## Examples

`AstraPhaseTests` verifies headless filesystem, timer, thread dispatch/pending tags, dynamic-library error behavior, file-watch polling, crash packet capture, and public header isolation.

## Troubleshooting

- Never put SDL includes or native handles in public headers.
- Dynamic library failures should become diagnostics or `Result` errors.

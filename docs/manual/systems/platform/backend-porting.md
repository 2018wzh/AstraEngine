# Platform Backend Porting

Status: Phase 1 implemented foundation.

## Overview

Platform exposes public services for window, filesystem, dynamic library, thread, timer, and crash capture. Phase 1 includes a facade library, independent backend DLL targets, a headless backend, SDL-backed desktop window backend compile path, target-platform descriptors, opaque dynamic library handles, watch polling, pending task tags, and crash capture context.

## Key Concepts

- Public headers do not expose SDL or OS/native handles.
- Headless backend is the CI and smoke-test default.
- SDL is confined to private implementation files.
- `TargetPlatformDesc` is the shared source of OS/architecture, binary naming, plugin bin directory, script extension, and backend capability flags.
- Mobile and Web targets are described for packaging and release-gate planning, but their runtime backends are stubs that return `Unsupported`.
- Dynamic libraries are represented publicly by `DynamicLibraryHandle`; native handles remain private.
- Crash capture can include build info, frame index, thread id, recent logs, and package/project hash.

## Architecture

See [Platform Services](../../../design/foundation-core-platform-property.md).

## Programming Guide

Implement the public service interfaces in `Astra/Platform/Platform.hpp`. Use `CreatePlatform(PlatformCreateDesc, DiagnosticSink&)` for new code and the legacy `CreateHeadlessPlatform()` / `CreateSdlPlatform()` wrappers for compatibility. Backend factories return `PlatformServices`; failures report diagnostics or `Result` errors rather than crashing. Use `FindTargetPlatform()` and `KnownTargetPlatforms()` instead of local platform tables in tools. Use `PollWatches()` for headless watch invalidation tests and `PendingTags()`/`Drain()` to prove thread queue shutdown behavior.

## API Reference

Use `Engine/Runtime/Platform/Public/Astra/Platform/Platform.hpp`.

Backend build targets are:

- `AstraPlatform`: public facade and target descriptor API.
- `AstraPlatformHeadless`: headless backend DLL target.
- `AstraPlatformDesktopSdl`: SDL desktop backend DLL target when SDL is enabled.
- `AstraPlatformMobileStub`: mobile backend stub DLL target.
- `AstraPlatformWebStub`: Web backend stub DLL target.

## Examples

`AstraPhaseTests` verifies headless filesystem, timer, thread dispatch/pending tags, dynamic-library error behavior, file-watch polling, crash packet capture, target-platform descriptors, unsupported mobile/Web backend diagnostics, and public header isolation.

## Troubleshooting

- Never put SDL includes or native handles in public headers.
- Do not duplicate target-platform tables in tools; use `FindTargetPlatform()`.
- Dynamic library failures should become diagnostics or `Result` errors.

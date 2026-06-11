# Core Programming Guide

Status: Phase 1 implemented foundation.

## Overview

Core provides dependency-light foundation APIs for diagnostics, diagnostic code registration, release gate policy, logging, error reporting, profiling markers, config layering/profile resolution, path/time helpers, versioned serialization, stable IDs, and build info.

## Key Concepts

- Core does not depend on Platform, SDL, Lua, VN, AI, Editor, renderer, audio, or Legacy.
- Diagnostics are machine-readable packets; logs are time-series observations.
- `DiagnosticCodeRegistry`, `ReleasePolicy`, and `FoundationGateReport` provide the Phase 1 Foundation release-gate contract.
- Config can be resolved for development/runtime/release profiles; release resolution excludes user overrides and produces a stable hash.
- Stable IDs are parsed and normalized strings, never pointers or ECS entities.
- Serialization uses schema/version headers, migration rules, and explicit unknown-field policies.

## Architecture

Primary design reference: [Foundation Core / Platform / Property](../../../design/foundation-core-platform-property.md).

## Programming Guide

Use `Astra::Core::Result<T>` for recoverable operations, `DiagnosticSink` for user-facing or release-blocking evidence, `DiagnosticCodeRegistry` plus `EvaluateFoundationGate()` for Foundation release-gate evaluation, `ErrorReporter` for developer/recoverable/fatal policy, `ProfilingCapture` for runtime-independent markers, and `StableId` aliases for persistent identifiers. Config layers merge from defaults toward command-line style overrides; `ResolveForProfile()` produces release-safe resolved JSON and a stable hash.

## API Reference

Public headers live in `Engine/Runtime/Core/Public/Astra/Core`.

## Examples

Compiled examples are in `Engine/Tests/PhaseTests.cpp`.

## Troubleshooting

- Do not add SDL, Lua, AI, VN, Editor, renderer, audio, or Legacy includes to Core.
- Use diagnostics for actionable failures and logs for observation.

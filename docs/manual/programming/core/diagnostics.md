# Diagnostics Guide

Status: Phase 1 implemented foundation.

## Overview

Diagnostics report structured problems that can be consumed by tests, tools, future Editor panels, MCP tools, and release gates.

## Key Concepts

- Severity values are `info`, `warning`, `error`, `blocking`, and `fatal`.
- `blocking` and `fatal` diagnostics block release-style validation.
- Registered diagnostic codes can also define a minimum release severity; `EvaluateFoundationGate()` uses that threshold and can reject unregistered codes.
- Diagnostics carry code, category, message, source location, related objects, context, and suggested fixes.

## Architecture

See [Foundation diagnostics](../../../design/foundation-core-platform-property.md).

## Programming Guide

Emit diagnostics through `Astra::Core::DiagnosticSink`. Register release-relevant codes with `DiagnosticCodeRegistry`, configure `ReleasePolicy`, and call `EvaluateFoundationGate()` to produce a `FoundationGateReport`. Use stable diagnostic codes and categories so CLI, future Editor, and MCP surfaces can show the same issue without rewriting it.

## API Reference

Use `Astra/Core/Diagnostics.hpp`.

## Examples

`AstraPhaseTests` verifies JSON serialization, release-blocking severity behavior, registered-code thresholds, and unregistered-code rejection. `astra validate . --strict --json` emits the `foundation_core_gate.gate_report` artifact.

## Troubleshooting

- Do not use free-text logs as a substitute for diagnostics.
- Use [Logging Guide](logging.md) when you need temporal context around diagnostics.
- Include object IDs when a diagnostic points at a schema, module, service, asset, or future runtime object.

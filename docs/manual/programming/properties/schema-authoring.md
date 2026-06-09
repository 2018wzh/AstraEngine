# Schema Authoring Guide

Status: Phase 1 implemented foundation.

## Overview

Schema authoring starts from `TypeDescriptor` and `PropertyDescriptor` metadata and emits JSON Schema for validation and tooling.

## Key Concepts

- Property kind maps to JSON Schema type plus Astra metadata.
- Required/default/range/regex metadata participates in schema output.
- Inspector metadata is preserved as Astra extension fields.
- Nested struct, array, map, and tagged-union descriptors can reference registered nested types.
- Schema version edges are registered separately and validated before migration-sensitive workflows.

## Architecture

See [Foundation PropertySystem](../../../design/foundation-core-platform-property.md).

## Programming Guide

Define descriptors in code, register them with `TypeRegistry`, then call `GenerateJsonSchema`. Register `SchemaVersionEdge` entries for expected version upgrades and use `EvaluateWrite()` when an Editor, AI, runtime, or release tool proposes a field change.

## API Reference

Use `Astra/PropertySystem/PropertySystem.hpp`.

## Examples

The Phase 1 tests show schema generation for localized text, integer fields, nested struct, array, map, tagged union, schema version validation, and write-policy rejection.

## Troubleshooting

- Keep generated schemas deterministic.
- Treat missing type descriptors as blocking diagnostics.

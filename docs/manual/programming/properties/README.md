# Property System Guide

Status: Phase 1 implemented foundation.

## Overview

PropertySystem provides lightweight descriptors for future Inspector, schema, serialization, review, MCP field editing, and release-gate workflows without implementing UE-style UObject reflection. Phase 1 now includes Foundation gate evidence for nested schema generation, schema version validation, write policy, and release-sensitive diff/audit output.

## Key Concepts

- Types and properties are described with stable IDs and metadata.
- Flags include AI-editable, tool-generated, read-only, requires-review, runtime-only, editor-only, and release-sensitive.
- JSON Schema generation covers scalar, struct, array, map, tagged union, localized text, asset ref, and enum-style descriptors.
- Write policy checks AI/editor/runtime/release writes against property flags.

## Architecture

See [PropertySystem](../../../design/foundation-core-platform-property.md).

## Programming Guide

Register `TypeDescriptor` values with `TypeRegistry`, generate JSON Schema, validate required/default fields, register schema version edges, evaluate writes with `EvaluateWrite()`, and apply migration steps for rename/default/deprecate flows.

## API Reference

Use `Engine/Runtime/PropertySystem/Public/Astra/PropertySystem/PropertySystem.hpp`.

## Examples

`AstraPhaseTests` registers nested struct/array/map/tagged union descriptors and validates schema/default/migration/write-policy/diff behavior.

## Troubleshooting

- Do not use PropertySystem to introduce Actor, Editor, AI provider, renderer, audio, or Legacy dependencies into Core.
- Mark review-sensitive fields with flags now so future release gates can enforce them.



# Migration

Status: Phase 0 scaffold. Migration systems are planned and not implemented.

## Overview

This section will document snapshot migration, schema migration, plugin ABI migration, save migration, and project/content migration.

## Key Concepts

- Migration is schema-versioned.
- Unknown fields follow explicit preserve, warn, error, or drop policy.
- Save/package critical schemas require stricter diagnostics than editable source documents.
- Legacy compatibility is an expansion track and does not redefine native runtime migration.

## Architecture

Primary design references:

- [Foundation Core / Platform / Property](../../design/foundation-core-platform-property.md)
- [Runtime Core](../../design/runtime-core.md)
- [Extension and Module System](../../design/extension-and-module-system.md)
- [Compatibility Layer](../../design/compatibility-layer.md)

## Programming Guide

Future pages should cover migration registry entries, property field rename/split/merge, default injection, deprecated fields, plugin ABI ranges, and save compatibility testing.

## API Reference

Planned references include schema IDs, versioned document headers, migration registry descriptors, module API version ranges, and save container headers.

## Examples

Planned examples include renaming a component field, migrating a plugin descriptor API range, and rejecting an incompatible save with diagnostics.

## Troubleshooting

- Do not silently drop source fields without an explicit migration rule.
- Do not claim old saves are compatible until migration tests exist.
- Keep migration diagnostics machine-readable.

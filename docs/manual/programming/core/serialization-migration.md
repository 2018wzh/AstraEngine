# Serialization And Migration Guide

Status: Phase 1 implemented foundation.

## Overview

Core serialization provides versioned document headers and migration registration for source, config, future save/package data, and diagnostics payloads.

## Key Concepts

- Documents carry `schema`, `version`, `object_id`, and `payload`.
- Migrations advance one schema version at a time.
- Unknown field policy is part of the migration contract and supports preserve, warn, error, and drop behavior.

## Architecture

See [Foundation serialization](../../../design/foundation-core-platform-property.md).

## Programming Guide

Register `MigrationRule` entries in `MigrationRegistry`, then migrate a `VersionedDocument` to the requested target version. Missing migration paths emit blocking diagnostics. Use `known_fields_after_migration` and `ApplyUnknownFieldPolicy()` when a migration needs release-gate evidence for forward-compatible or critical schemas.

## API Reference

Use `Astra/Core/Serialization.hpp`.

## Examples

`AstraPhaseTests` includes sequential migration and preserve/warn/error/drop unknown-field policy examples.

## Troubleshooting

- Treat missing migration paths as blocking for runtime/save/package critical schemas.
- Do not silently drop unknown fields unless an explicit migration rule says to.



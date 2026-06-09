# Property Migration Cookbook

Status: Phase 1 implemented foundation.

## Overview

Property migrations currently support field rename, default injection, and deprecated field removal helpers.

## Key Concepts

- Migrations operate on JSON values.
- Defaults are injected only when the target field is missing.
- Deprecated fields are removed explicitly.

## Architecture

See [Serialization And Migration](../../../design/foundation-core-platform-property.md).

## Programming Guide

Build a vector of `MigrationStep` entries and call `TypeRegistry::ApplyMigration`.

## API Reference

Use `Astra/PropertySystem/PropertySystem.hpp`.

## Examples

`Astra_Phase1Tests` migrates `name` to `display_name` and injects `age`.

## Troubleshooting

- Keep migration steps explicit and reviewable.
- Do not silently reinterpret unrelated fields.

# Engine Module Slot Guide

Status: Phase 1 implemented foundation.

## Overview

Engine module slots record explicit provider candidates for replaceable engine capabilities. Phase 1 implements provider registration metadata and foundation policy validation; UI-driven project selection remains planned.

## Key Concepts

- Slots do not replace `ServiceRegistry` or `ExtensionRegistry`.
- Provider IDs must be unique.
- Phase 1 validates explicit slot/provider policy references and rejects provider-slot mismatches.

## Architecture

See [EngineModuleSlot](../../../design/extension-and-module-system.md).

## Programming Guide

Register a provider through `AstraEngineModuleRegistryApi::register_provider` during module initialization. Use `EngineModuleRegistry::ValidatePolicy()` to validate selected providers against slots in foundation release-gate style checks.

## API Reference

Use `Astra/ModuleRuntime/ModuleRuntime.hpp` and `Astra/ModuleRuntime/ModuleAbi.h`.

## Examples

`Phase1Example` registers `astra.phase1.example.renderer2d` for `astra.renderer2d`.

## Troubleshooting

- Do not use slot registration to override non-replaceable core systems.
- Do not infer provider priority from load order.
- Treat provider-slot mismatches as release-blocking diagnostics.



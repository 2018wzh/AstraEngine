# Plugin Authoring Guide

Status: Phase 1 implemented foundation.

## Overview

Phase 1 plugins use YAML descriptors and a dynamic module binary to register foundation services, extensions, and engine module providers. Foundation validation now produces a module release-gate report with descriptor policy checks and binary hash evidence.

## Key Concepts

- Descriptor validation happens before binary load.
- Entrypoints must stay inside the plugin root.
- Dependencies are resolved before module initialization.
- Invalid descriptors produce blocking diagnostics.
- `ValidateModuleReleaseGate()` checks descriptor policy, dependency order, packaged safety, entrypoint existence, and module binary evidence.
- `ServiceRegistry::Resolve()` records service resolve audit decisions against module state, capability, version, and permission.

## Architecture

See [Extension and Module System](../../../design/extension-and-module-system.md).

## Programming Guide

Create a `*.plugin.yaml` with plugin id, version, Astra API range, module id, entrypoint, capabilities, permissions, diagnostics prefix, and release metadata. Implement the C ABI entrypoint in the module binary. Repository validation exposes the example plugin's module release-gate report through `foundation_core_gate.module_release_gate`.

## API Reference

Use `Astra/ModuleRuntime/ModuleRuntime.hpp` and `Astra/ModuleRuntime/ModuleAbi.h`.

## Examples

The Phase 1 example plugin descriptor is generated under `build/Plugins/Phase1Example` and demonstrates a runtime service, an `AssetImporter` extension, an `astra.renderer2d` provider registration, and release-gate binary SHA-256 evidence.

## Troubleshooting

- Descriptor paths that escape the plugin root are rejected.
- Required dependency cycles or missing dependencies are blocking diagnostics.

# Module ABI Reference

Status: Phase 1 implemented foundation.

## Overview

Modules enter AstraEngine through a C ABI function named `astra_module_main`.

## Key Concepts

- ABI version is currently `1`.
- ABI strings are UTF-8 pointer and length pairs.
- Host services are exposed through callback tables.
- Module lifetime is initialize, activate, deactivate, shutdown, unload.
- Dynamic module binaries are checked by the Foundation module release gate before being treated as packaged-safe evidence.

## Architecture

See [AstraModule C ABI](../../../design/extension-and-module-system.md).

## Programming Guide

Export `astra_module_main`, fill `AstraModuleApi`, and register services/extensions/providers during initialize. Return result codes and emit diagnostics through the host API. The C++ host wraps loaded binaries behind opaque platform dynamic library handles; native handles never cross the public ABI.

## API Reference

Use `Engine/Runtime/ModuleRuntime/Public/Astra/ModuleRuntime/ModuleAbi.h`.

## Examples

See `Engine/Plugins/Examples/Phase1Example/Source/Phase1Example.cpp`.

## Troubleshooting

- Do not pass STL ownership, C++ Actor pointers, SDL/native handles, renderer/audio handles, or Editor widgets across the ABI.
- Missing lifecycle callbacks are treated as blocking ABI errors.
- Missing entrypoints, bad ABI versions, and missing release-gate binary evidence are reported as machine-readable diagnostics.

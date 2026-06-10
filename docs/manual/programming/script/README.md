# Script Foundation

Status: Phase 4 implemented foundation.

## Overview

`AstraScript` provides the Phase 4 foundation `ScriptRuntimeHost` for Astra Native DSL and Lua scripts. Both paths compile into a shared command stream and affect the world only through `RuntimeEvent` and `PresentationCommand` DTOs.

## Key Concepts

- Native DSL and Lua use the same `ScriptEventBridge`.
- Script state enters a `ScriptSnapshot` with active label, command index, variables, choice state, and executed command IDs.
- Lua uses `sol2` over the vcpkg Lua runtime, but public headers do not expose Lua or `sol2` types.
- Foundation Lua exposes only the `astra` host table; filesystem, network, OS, package, debug, and native handle access are not part of the host API.

## Architecture

Design reference: [Script and Presentation](../../../design/script-and-presentation.md).

`AstraScript` depends on Core, Asset, Media, Runtime, and Scene through public DTOs. It does not depend on Editor, AI providers, MCP servers, legacy runtimes, SDL, renderer handles, or audio handles.

## Programming Guide

Create a `ScriptRuntimeHost`, compile a `ScriptSource`, then run it against a `RuntimeWorld`:

```cpp
Astra::Script::ScriptRuntimeHost host;
auto compiled = host.CompileNative(source, diagnostics);
auto result = host.Run(compiled.Value(), runtime, {"opening", 0}, diagnostics);
```

Use `CompileLua()` for Lua sources that call the Phase 4 `astra` host table.

## API Reference

- `Engine/Runtime/Script/Public/Astra/Script/Script.hpp`
- `Astra::Script::ScriptRuntimeHost`
- `Astra::Script::ScriptEventBridge`
- `Astra::Script::CompiledScript`
- `Astra::Script::ScriptSnapshot`

## Examples

`Samples/NativeVN/Content/Scripts/opening.astra` and `opening.lua` produce equivalent Phase 4 headless presentation hashes.

## Troubleshooting

- Phase 4 does not implement the production debugger, hot reload rollback, Graph/Timeline compiler, or full Lua continuation snapshot.
- Script diagnostics should include file, line, column, and suggested fixes for authoring errors.

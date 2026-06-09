# Lua Host API Foundation

Status: Phase 4 implemented foundation.

## Overview

Phase 4 Lua support proves that a mature Lua runtime can drive the same VN event and presentation path as Native DSL. The implementation uses vcpkg Lua plus `sol2` privately.

## Key Concepts

- Scripts call the `astra` table instead of renderer, audio, filesystem, or runtime internals.
- Foundation host functions mirror Native DSL commands.
- Lua output is compared against Native DSL output through headless hashes.
- Public Astra headers do not expose Lua or `sol2`.

## Architecture

Lua is hosted by `Astra_Script`, not Core. The host compiles Lua-authored commands into `CompiledScript`, then `ScriptEventBridge` emits Runtime and Media DTOs.

## Programming Guide

Example:

```lua
astra.label("opening")
astra.bg("native:/Backgrounds/Room")
astra.show("alice", "native:/Characters/Alice/Normal", "center")
astra.say("alice", "Good morning.", "native:/Voice/Alice/opening_001")
astra.choice("Walk together", "route_walk")
```

## API Reference

- `Astra::Script::ScriptRuntimeHost::CompileLua`
- `Astra::Script::FoundationScriptProviders`
- `Astra::Script::LuaRuntimeId`

## Examples

`Samples/NativeVN/Content/Scripts/opening.lua` is the Phase 4 Lua parity source.

## Troubleshooting

- Phase 4 Lua is a foundation host path, not the full production sandbox/debugger.
- Use only the `astra` host API table.
- Do not rely on filesystem, OS, package loading, debug hooks, wall-clock decisions, or provider-private state.

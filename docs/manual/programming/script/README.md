# Script Runtime

Status: Phase 8 Script/AstraVN completion slice implemented.

## Overview

`AstraScript` now treats `.astra` as the production VN-first authoring DSL. It compiles source into a shared `CompiledScript` containing a document AST, state graph IR, narrative IR, effect graph IR, source map, debug symbols, command manifest, extension manifest, and a v2 script snapshot model.

Lua remains private behind `sol2`, but its Phase 8 role is extension/system command schema registration, not ordinary story authoring.

## Key Concepts

- `.astra` requires stable `#@id` markers for save/replay/debug-relevant nodes.
- Scenes must end explicitly with `->`, `return`, `end`, or `await`; implicit fallthrough is a blocking diagnostic.
- Script execution affects the world through `RuntimeEvent`, scheduler waits, and `PresentationCommand`.
- `ScriptSnapshot v2` stores active story/state/scene/timeline, command IDs, variables, waits, stage state, timeline state, and choice state.
- Built-in AstraVN commands and future extension commands share the same command schema registry.
- Lua extension packages register typed command schemas through the sandboxed `aivn` table. Lua story scripts are removed.

## Architecture

Design reference: [Script and Presentation](../../../design/script-and-presentation.md).

`AstraScript` depends on Core, Asset, Media, Runtime, and Scene through public DTOs. It does not expose Lua, `sol2`, SDL, renderer handles, audio handles, Editor objects, AI providers, MCP servers, or legacy runtimes.

## Programming Guide

```cpp
Astra::Script::ScriptRuntimeHost host;
auto compiled = host.CompileNative(source, diagnostics);
auto result = host.Run(compiled.Value(), runtime, {"station", 0}, diagnostics);
auto step = host.Step(compiled.Value(), result.snapshot, runtime, diagnostics);
```

Use `CompileLuaExtensionPackage()` for Lua extension schema files. `CompileLua()` is a compatibility symbol that now emits `ASTRA_SCRIPT_LUA_STORY_REMOVED`.

## API Reference

- `Engine/Runtime/Script/Public/Astra/Script/Script.hpp`
- `Astra::Script::ScriptRuntimeHost`
- `Astra::Script::CompiledScript`
- `Astra::Script::ScriptDocument`
- `Astra::Script::StateGraphIr`
- `Astra::Script::NarrativeIr`
- `Astra::Script::EffectGraphIr`
- `Astra::Script::ScriptSourceMap`
- `Astra::Script::ScriptSnapshot`
- `Astra::Script::ScriptCommandSchema`

## Examples

`Samples/NativeVN/Content/Scripts/opening.astra` is the Phase 8 production DSL sample. `opening.lua` registers a schema-first `live2d.motion.play` extension command fixture.

## Troubleshooting

- `ASTRA_SCRIPT_STABLE_ID_REQUIRED` means an executable node needs `#@id`.
- `ASTRA_SCRIPT_SCENE_FALLTHROUGH` means a scene lacks an explicit terminator.
- `ASTRA_SCRIPT_COMMAND_SCHEMA_UNKNOWN` means a command or extension has no registered schema.
- `ASTRA_SCRIPT_LUA_STORY_REMOVED` means a Lua file was sent through the removed story runtime path.
- Lua files must use the `aivn.extension()` and `aivn.command()` sandbox API.

# Native DSL Foundation

Status: Phase 4 implemented foundation.

## Overview

The Phase 4 Native DSL is a small VN-first text format used to prove the ScriptRuntimeHost and AstraVN event bridge. It is not the final production language.

## Key Concepts

- Supported commands are `label`, `bg`, `show`, `say`, `choice`, `jump`, `set`, `get`, `audio`, and `filter`.
- Asset references use stable URIs such as `native:/Backgrounds/Room`.
- Choices route to labels and become VN choice events plus UI presentation DTOs.
- Dialogue can carry a logical voice asset that becomes an audio presentation command.

## Architecture

Native DSL compiles to the same command stream used by Lua. Runtime execution emits `RuntimeEvent` records and `PresentationCommand` records; Media remains responsible only for logical headless rendering in Phase 4.

## Programming Guide

Example:

```text
label opening
bg native:/Backgrounds/Room
show alice native:/Characters/Alice/Normal center
say alice "Good morning." voice native:/Voice/Alice/opening_001
choice "Walk together" -> route_walk
```

## API Reference

- `Astra::Script::ScriptRuntimeHost::CompileNative`
- `Astra::Script::ScriptCommand`
- `Astra::Script::ScriptDebugSymbol`

## Examples

Run `astra validate Samples/NativeVN --strict --json` and inspect the `phase4_script_vn.native` artifact.

## Troubleshooting

- Missing labels and invalid asset URIs are blocking diagnostics.
- Use quoted dialogue and choice text.
- Do not treat this foundation grammar as the final Astra authoring language.

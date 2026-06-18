# Lua Extension SDK

Status: Phase 8 sandboxed extension schema path implemented.

## Overview

Lua is hosted by `AstraScript` as an extension/system implementation layer. Ordinary story authors should use `.astra`; Lua packages register typed, versioned command schemas consumed by the DSL compiler and tooling.

## Key Concepts

- Lua runs through private `sol2`; public Astra headers do not expose Lua types.
- The sandbox removes raw filesystem, OS, package loading, debug access, network, native handles, and direct world mutation.
- Extension packages call `aivn.extension(id, version)` and `aivn.command(command_id, schema)`.
- Command schemas declare params, execution/save/skip/rollback policy, determinism, channels, and editor metadata.

## Example

```lua
aivn.extension("live2d", "1.0.0")

aivn.command("motion.play", {
  version = 1,
  params = {
    actor = { type = "ActorRef", required = true }
  },
  execution = {
    deterministic = true,
    save = "serializable",
    skip = "finish",
    rollback = "snapshot"
  },
  editor = { label = "Live2D Motion" }
})
```

## API Reference

- `Astra::Script::ScriptRuntimeHost::CompileLuaExtensionPackage`
- `Astra::Script::ScriptExtensionCommandSchema`
- `Astra::Script::LuaExtensionRuntimeId`
- `Astra::Script::LuaRuntimeId` remains a compatibility alias for the extension package runtime, not a story VM.

## Troubleshooting

- `io`, `os`, `package`, and `debug` are not available.
- Non-deterministic command schemas are blocking in deterministic VN script evidence.
- `CompileLua()` and `VnSession::RunLua()` reject Lua story execution; use `.astra` for narrative flow.

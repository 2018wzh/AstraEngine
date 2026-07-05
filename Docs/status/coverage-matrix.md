# Implementation Coverage Matrix

| 模块 | Design | Contract | Public API | Data Format | Test Scenario | Release Gate | Manual |
| --- | --- | --- | --- | --- | --- | --- | --- |
| EngineCore | [module](../modules/engine-core.md) | [runtime](../contracts/runtime.md) | `astra-runtime` planned | save/package container | `native_smoke.yaml` | runtime determinism | [operator](../manual/operator-guide.md) |
| Plugin ABI | [architecture](../product/architecture.md) | [plugin](../contracts/plugin-abi.md) | `astra-plugin` planned | plugin YAML | plugin load/unload scenario | fingerprint/capability | [plugin guide](../manual/plugin-developer-guide.md) |
| Asset Pipeline | [module](../modules/asset-pipeline.md) | [data](../contracts/data-formats.md) | `astra-asset`, `astra-cook` planned | asset sidecar YAML, binary package | import/cook/package scenario | package integrity | [creator](../manual/creator-manual.md) |
| Media Runtime | [module](../modules/media-runtime.md) | [media](../contracts/media.md) | provider traits planned | FilterGraph YAML, AudioGraph YAML | media decode/render scenario | decode/provider gate | [operator](../manual/operator-guide.md) |
| AstraVN | [module](../modules/astra-vn.md), [script spec](../modules/astra-vn-script.md) | [script-vn](../contracts/script-vn.md) | `astra-vn` planned | `.astra`, policy bundle, compiled story IR | [full VN playthrough](../samples/astra-vn-script/full_playthrough.yaml) | VN release profile, Lua policy gate | [creator](../manual/creator-manual.md) |
| AstraEditor | [module](../modules/editor.md) | [AI/MCP](../contracts/ai-mcp.md) | Qt/Rust bridge planned | layout preset YAML | creator workflow scenario | editor package gate | [creator](../manual/creator-manual.md) |
| AI/MCP | [module](../modules/ai-mcp.md) | [AI/MCP](../contracts/ai-mcp.md) | provider/tool traits planned | audit log, AI draft sidecar | trusted session scenario | provider-free replay | [plugin guide](../manual/plugin-developer-guide.md) |
| AstraEMU | [module](../modules/astra-emu.md), [family research](../emu/README.md) | [IPC](../contracts/astraemu-ipc.md) | manager/core RPC planned, research tools in `Tools/AstraEMU` | local case report, legacy archive formats | family full-flow scenario | local report gate | [operator](../manual/operator-guide.md) |
| Platforms | [platforms](../platforms/README.md) | [release](../contracts/release-gate.md) | host APIs planned | capability report | platform smoke scenario | platform eligibility | [operator](../manual/operator-guide.md) |

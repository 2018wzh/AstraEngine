# Script and AstraVN Contract

`.astra` 是 AstraVN 的 canonical story source。Lua 5.4 通过 `mlua` 提供扩展、逻辑和 EMU patch/decode runtime；Lua 不是 EngineCore 依赖。

## `.astra` 输出

编译器输出：

```rust
pub struct CompiledStory {
    pub state_graph: StateGraphIr,
    pub narrative: NarrativeIr,
    pub effects: EffectGraphIr,
    pub source_map: SourceMap,
    pub debug_symbols: DebugSymbols,
    pub command_manifest: CommandManifest,
}
```

Graph 和 Timeline 保存作者元数据，必须能编译到同一 IR。Editor 不能维护第二套 runtime model。

## 商业 VN 基线

AstraVN v1 覆盖 dialogue、choice、variables、call/return、backlog、auto、skip、read-state、save/load、config、voice replay、movie、常见 transition、screen effects、message window、route flags 和 timed delay blocks。

## Lua Capability Sandbox

Lua 默认无文件、网络或系统调用。能力通过 descriptor 声明：

```yaml
lua:
  runtime: lua54
  capabilities:
    - astra.vn.command_extension
    - astra.emu.patch.decode
  fs:
    read_roots: [foreign-content]
  network: false
```

AstraEMU 的 Lua patch/decode API 只提供本地结构、索引、压缩、用户授权 key 输入和 payload transform。遇到 DRM、商业保护或访问控制必须返回 blocking diagnostic。

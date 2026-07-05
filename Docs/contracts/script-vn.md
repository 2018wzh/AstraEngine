# Script and AstraVN Contract

`.astra` 是 AstraVN 的 canonical story source。AstraVN Core 固化 dialogue、choice、backlog、save/load、read-state、voice replay 等权威语义；Lua 5.4 通过 `mlua` 提供策略层能力，用于表现、系统页、复杂演出和插件组合。

完整脚本规格见 [AstraVN Script Spec](../modules/astra-vn-script.md)。

## `.astra` 输出

编译器输出：

```rust
pub struct CompiledStory {
    pub story_manifest: StoryManifest,
    pub system_manifest: SystemStoryManifest,
    pub variable_manifest: VariableManifest,
    pub command_manifest: CommandManifest,
    pub lua_manifest: LuaPolicyManifest,
    pub timeline_ir: TimelineIr,
    pub text_effect_ir: TextEffectIr,
    pub source_map: SourceMap,
    pub debug_symbols: DebugSymbols,
}
```

Graph 和 Timeline 保存作者元数据，必须能编译到同一 IR。Editor 不能维护第二套 runtime model。

## 商业 VN 基线

AstraVN v1 覆盖 dialogue、choice、variables、call/return、backlog、auto、skip、read-state、save/load、config、gallery、replay、route chart、voice replay、movie、常见 transition、screen effects、message window、route flags 和 timed delay blocks。

官方 Lua 策略包覆盖 message UI、choice UI、system stories、timeline presets、localization UI 和常用演出。第三方策略包可以替换表现和系统流程，不能破坏 Core save/backlog/read-state 语义。

## Lua Capability Sandbox

Lua 默认无文件、网络或系统调用。权威写入必须通过记录型 `astra.mutate` API，产生 trace、rollback、dirty scope 和 replay event。能力通过 descriptor 声明：

```yaml
lua:
  runtime: lua54
  capabilities:
    - astra.vn.command_extension
    - astra.vn.policy_bundle
    - astra.emu.patch.decode
  fs:
    read_roots: [foreign-content]
  network: false
```

AstraEMU 的 Lua patch/decode API 只提供本地结构、索引、压缩、用户授权 key 输入和 payload transform。遇到 DRM、商业保护或访问控制必须返回 blocking diagnostic。

# Script and AstraVN Contract

`.astra` 是 AstraVN 的 canonical story source。AstraVN Core 固化 dialogue、choice、backlog、save/load、read-state、voice replay 等权威语义；Luau 通过 `mlua` 提供策略层能力，用于表现、系统页、复杂演出和插件组合。

完整脚本规格见 [AstraVN Script Spec](../modules/astra-vn-script.md)。演出模型、标准命令库和系统 UI 分别见 [AstraVN Presentation Model](../modules/astra-vn-presentation-model.md)、[AstraVN Standard Command Library](../modules/astra-vn-standard-commands.md) 和 [AstraVN System UI Profile](../modules/astra-vn-system-ui-profile.md)。

## `.astra` 输出

编译器主线从 source 进入 frontend，再 lowering 到 runtime IR：

```text
.astra source
  -> Lexer
  -> TokenStream
  -> Lossless CST
  -> Typed AST
  -> Semantic Passes
  -> CompiledStory
```

当前 Rust baseline 的 `CompiledStory` 是：

```rust
pub struct CompiledStory {
    pub schema: String,
    pub story_hash: Hash128,
    pub story_manifest: StoryManifest,
    pub variable_manifest: VariableManifest,
    pub command_manifest: CommandManifest,
    pub system_story_manifest: SystemStoryManifest,
    pub stories: Vec<Story>,
    pub states: BTreeMap<String, State>,
    pub route_graph: RouteGraph,
    pub source_map: BTreeMap<String, SourceRef>,
    pub debug_symbols: BTreeMap<String, String>,
}
```

Graph 和 Timeline 保存作者元数据，必须能编译到同一 IR。Editor 不能维护第二套 runtime model。`luau_manifest`、`timeline_ir`、`text_effect_ir`、token span、attribute span、macro expansion stack 和 command-level source map 是 migration target，不是当前 implemented contract。

`compile_astra_sources` 当前只接受 Core 和内置 standard command。扩展命令必须通过 `CompileAstraOptions::bind_extension(ExtensionCommandDescriptor)` 绑定 provider、schema 和逐字段类型；只给 command/provider 字符串的旧入口已经删除。`ExprBytecode`、macro expansion stack 和 package-bound extension provider execution 仍是 migration target，不能写成当前能力。

Story 与 UI source role 已统一到 `compile_astra_project(...) -> CompiledVnProject`，公开 `compile_astra_sources`、`compile_astra_sources_with_options` 和旧 `vn.compiled_story` reader 已删除。UI role 与 Story role 共享 Lexer/CST/formatter/source-map，但使用独立 Typed AST 和 semantic passes；详细契约见 [UI Contract](ui.md) 与 [ADR 0016](../adr/0016-astravn-script-declared-ui.md)。

## 商业 VN 基线

AstraVN v1 覆盖 dialogue、choice、variables、call/return、backlog、auto、skip、read-state、save/load、config、gallery、replay、route chart、voice replay、movie、常见 transition、screen effects、message window、route flags 和 timed delay blocks。

官方 Luau 策略包覆盖 message UI、choice UI、system stories、timeline presets、localization UI、常用演出和 command provider binding。Stage 3 package 会写入 `vn.policy_bundle_manifest` 与 `vn.policy_bundle_source_cache`，release report 只记录 source hash、byte size、section id 和 diagnostic，不输出 Luau source payload。第三方策略包可以替换表现和系统流程，不能破坏 Core save/backlog/read-state 语义。

## Presentation Profiles

`vn.commercial_baseline` 和 `vn.system_ui_profile` 是普通商业 VN 发布的阻断 gate。`vn.advanced_presentation` 是项目显式启用的高表现 profile，用于验证多层 stage、camera、video layer、shader/filter、voice sync、复杂 text effect、Timeline join/cancel、skip/auto/replay 和 fallback。

## Command Provider Contract

命令解析必须经过显式 provider binding。Core command 由 AstraVN Core 提供；standard presentation command 由 standard command provider 提供；extension command 由项目 manifest 或插件 provider 绑定。开发 profile 可以把 unknown command 报为 warning 或受控 diagnostic，release profile 必须 blocking，不能把 unknown keyword 静默写成 presentation command。

Standard command 在编译期直接降为 `StageCommand`。viewport、layer、camera、movie、audio、timeline、effect 等字段使用 enum、整数或 `FixedScalar`，不再把 `BTreeMap<String, String>` 带入 Runtime。编译器会阻断未知字段、缺少必填字段、非法 enum/bool/decimal、非规范 `asset:/` URI、乱序或不足两个 keyframe，以及缺 fence/fallback 的 blocking wait。

扩展命令使用 `ExtensionCommandDescriptor` 声明 provider id、schema、字段名、字段类型和 required 状态。运行产物保存 descriptor 绑定和 typed payload。Player 没有装载对应 provider 时必须在 package open 阶段阻断，不能把扩展命令当成普通 Stage command 或忽略。

## Luau Capability Sandbox

Luau 默认无文件、网络或系统调用。权威写入必须通过记录型 `astra.mutate` API，产生 trace、previous value、rollback scope、dirty scope 和 replay event，并可在 rollback/playback 中恢复。`astra.command`、`astra.query` 和 `astra.trace` 只记录可序列化 capability request、read-only query 和 diagnostic event。`astra.snapshot` 只允许 nil、bool、integer、string 和 object 这类可序列化值；function、thread、userdata、native handle 或超出边界的 number 必须阻断，不能进入 save/replay。能力通过 descriptor 声明：

```yaml
luau:
  runtime: luau
  capabilities:
    - astra.vn.command_extension
    - astra.vn.policy_bundle
    - astra.emu.patch.decode
  fs:
    read_roots: [foreign-content]
  network: false
```

AstraEMU 的用户脚本统一使用 Luau。Trusted Project Profile 可以开启 read-only VFS、patch overlay、decode transform、text/media hook、VM trace、diagnostic 和 deterministic effect intent；状态注入只能在 fixed tick 边界变成 `LegacyEffect`、Blackboard、input 或 tag intent。AstraEMU 只处理本地结构、索引、压缩、用户授权 key 输入和 payload transform。脚本请求未授权 key 提取、商业保护处理或访问控制规避时，Manager 隔离禁用该脚本，并按无补丁模式继续 case。旧引擎研究文档中的 Lua/TJS 名称只描述 legacy engine 原有机制。

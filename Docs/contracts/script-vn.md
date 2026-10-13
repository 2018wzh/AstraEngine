# Script and AstraVN Contract

`.astra` 是 AstraVN 的 canonical story source。AstraVN Core 固化 dialogue、choice、backlog、save/load、read-state、voice replay 等权威语义；Luau 通过 `mlua` 提供策略层能力，用于表现、系统页、复杂演出和插件组合。

完整脚本规格见 [AstraVN Script Spec](../modules/astra-vn-script.md)。演出模型、标准命令库和系统 UI 分别见 [AstraVN Presentation Model](../modules/astra-vn-presentation-model.md)、[AstraVN Standard Command Library](../modules/astra-vn-standard-commands.md) 和 [AstraVN System UI Profile](../modules/astra-vn-system-ui-profile.md)。

演出帧推进、失败终止和 snapshot v8/v4 的具体边界见[演出帧推进迁移](presentation-tick-migration.md)。

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

## Luau 与宿主 IO

Luau 是可信游戏脚本，不把不可信 sandbox 作为产品目标。当前 bootstrap 仍使用受限 Lua 环境；实际 IO 通过 embedding host 的显式接口异步提交，不因可信脚本契约而假装任意内置文件/网络 API 已接通。保存只接受显式可序列化数据，不保存 function、thread、userdata 或 native handle。

变量写入、query、trace 与 snapshot 的现有实现及预算见 [Luau 实现](../implementation/luau-policy.md)。读档、重开或退出使旧异步结果失效，预览 seek 不重复外部副作用。

AstraEMU 使用各核心的原生脚本与 VM，不强制改为 Luau，也不经 Engine Blackboard 或 product provider 注入状态。Family 自持文件、媒体、渲染与存档，见 [EMU 契约](astraemu-ipc.md)。

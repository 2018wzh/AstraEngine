# AstraVN Script Spec

AstraVN 脚本层采用“Core 语义稳定，Luau 策略可换”的分工。`.astra` 仍是 canonical source；Luau 不是另一套剧情格式，而是策略层语言，用来定义表现、系统页和复杂演出。

## 分层

| 层 | 职责 | 不做什么 |
| --- | --- | --- |
| AstraVN Core | dialogue、choice、backlog、save/load、read-state、voice replay、变量域、AwaitToken/Fence、Mutation API | 不决定项目的 UI 风格、演出套路或系统页流程 |
| Rust Plugin | FilterGraph、AudioGraph、TextLayout、renderer、decode、native capability、性能敏感节点 | 不保存剧情权威状态，不跨 ABI 暴露 native handle |
| Luau Policy | message/choice UI、title/config/gallery/replay/chart、timeline preset、复杂演出、fallback、Editor metadata | 不破坏 Core save/backlog/read-state 语义 |

Core 提供完整 VN 权威语义。官方 Luau 策略包提供完整商业 VN 体验，第三方策略包可以替换表现和系统流程，但不能重写存档、回放、已读、backlog 等语义边界。

演出舞台、标准命令和系统页数据模型分别由 [AstraVN Presentation Model](astra-vn-presentation-model.md)、[AstraVN Standard Command Library](astra-vn-standard-commands.md) 和 [AstraVN System UI Profile](astra-vn-system-ui-profile.md) 定义。`CompiledStory` 通过 Runtime `StateMachine` 推进的规则见 [AstraVN StateMachine Playback](../implementation/astra-vn-state-machine.md)。本页只保留脚本核心契约。

## Compiler Frontend

AstraVN script v1 的主线是编译器前端标准化，不是 machine-code backend。当前实现已有 line-based parser/compiler baseline；新标准要求逐步迁到以下管线：

```text
.astra source
  -> Lexer
  -> TokenStream
  -> Lossless CST
  -> Typed AST
  -> Semantic Passes
  -> CompiledStory
```

`Lexer` 负责 token、quote、arrow、indent、`#@id`、attribute、comment 和 blank line。Lossless CST 保留 trivia、span 和原始结构，供 Editor、formatter、LSP 和 round-trip 使用。Typed AST 表达 story、state、scene、text、choice、option、jump、call、return、mutate、system page、stage 和 timeline。Semantic passes 负责 symbol collection、route resolving、variable validation、text key validation、system story validation、command provider resolving 和 macro lowering。

Runtime 不执行 `.astra` source。Editor 也不能维护第二套 runtime model。`.astra`、Graph、Timeline 和 Luau metadata 必须 lowering 到同一个 `CompiledStory` 或其后续扩展 IR，再进入 Runtime、package、release gate 和 player evidence。

Cranelift 不进入 v1 主线依赖。后续只有在表达式 bytecode 已经存在、且 profiling 证明 interpreter 是瓶颈时，才可以用 optional feature 形式尝试 JIT；package、save、replay hash 和 no-JIT 平台仍以 portable bytecode/IR 为准。

## `.astra` Source

`.astra` 使用缩进块和具名命令，不把旧引擎 tag 语法作为一等语法。传统引擎的优势进入语义和策略，不进入旧语法兼容承诺。

```astra
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    stage viewport:1920x1080 safe_area:16:9 #@id stage.room
    layer id:bg kind:background z:0 blend:normal #@id layer.bg
    layer id:characters kind:sprite z:100 blend:normal #@id layer.characters
    background asset:asset:/background/room layer:bg preset:soft_fade duration:300 #@id bg.room
    show id:hero asset:asset:/character/hero pose:normal layer:characters at:center preset:hero_enter #@id hero.enter
    voice asset:asset:/voice/hero0001 sync:text #@id voice.hero.0001
    text key:prologue.hello speaker:hero voice:voice.hero.0001 #@id line.hello
    effect text:cinematic.reveal lip_sync:true filter:soft_glow fallback:plain_reveal budget_ms:2 #@id effect.reveal
    choice key:prologue.where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop
```

Macro 是后续编译期机制。实现时必须让展开结果保留 source map、参数来源、展开栈和 debug symbol；当前 frontend 不接受 macro，不能把设计示例当成已实现能力。

## Core Semantics

当前 Rust baseline 中，Core 输出的 runtime IR 是：

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

`luau_manifest`、`timeline_ir`、`text_effect_ir`、token span、attribute span、macro expansion stack 和 command-level source map 是 frontend migration target。它们只有在 Rust schema、tests、package section 和 release gate 都落地后，才可以写成 implemented behavior。

运行时不直接执行 `.astra` source。`CompiledStory` 进入 `VnRuntimeState` 和 `VnCommandCursor` 后，由单个故事推进 `StateMachine` 调用 `astra.vn.step` action；角色、stage layer、Timeline track 和系统页都不是独立剧情状态机。

变量分四域：

| 域 | 生命周期 | 写入规则 |
| --- | --- | --- |
| `project` | 随 save/load 回滚 | Luau 可通过 `astra.mutate` 写入 |
| `global` | 跨 save/load 和路线保留 | 只能由显式 command 或授权策略写入 |
| `temp` | scene/story 临时状态 | Luau 可写，默认不进 save |
| `system` | 引擎和设置状态 | 只允许声明过的 system command 写入 |

所有权威写入都走 `astra.mutate`。直接改 Luau table 只能改策略私有缓存，不能改变 Runtime state。

## Luau Mechanism API

Luau 通过 `mlua` 运行。AstraVN 给 Luau 足够机制，不内置复杂演出策略。

```luau
astra.command.register(name, manifest, handler)
astra.command.filter(name, filter)
astra.command.emit(name, params)
astra.command.enqueue(name, params)

astra.mutate.set_var(scope, key, value)
astra.mutate.push_backlog(entry)
astra.mutate.presentation(command)
astra.mutate.audio(command)
astra.mutate.timeline(task)

astra.var.get(scope, key)
astra.query.text(key, locale)
astra.query.asset(id)
astra.query.backlog()
astra.query.savepoint()
astra.query.layout(target)
astra.trace.event(kind, fields)
astra.trace.performance_scope(name)
```

Lifecycle hook 全开放，但写入必须走记录型 Mutation API：

```luau
policy:on("load", fn)
policy:on("compile_preview", fn)
policy:on("runtime_start", fn)
policy:on("story_enter", fn)
policy:on("before_command", fn)
policy:on("after_command", fn)
policy:on("tick", fn)
policy:on("render_frame", fn)
policy:on("audio_frame", fn)
policy:on("save_snapshot", fn)
policy:on("release_check", fn)
```

`render_frame` 和 `audio_frame` 可以提交可回放的 presentation/audio command，不能读取平台 native handle、墙钟或回调顺序。所有随机数、时间、输入和 provider 结果都必须来自 Runtime 提供的 deterministic source。

Luau snapshot 只接受 table、number、string、bool、stable ref。function、thread、userdata、native handle 和 coroutine 状态不能进入发布包；Release Gate 遇到这些值必须阻断。

## Policy Bundle

复杂演出插件采用 Rust 机制、Luau 策略：

```yaml
schema: astra.policy_bundle.v1
id: com.example.cinematic
version: 0.1.0
engine_version: 0.1.0
rust_plugin: com.example.cinematic_nodes
luau_entry: cinematic_policy.luau
provides:
  commands:
    - astra.cinematic.reveal_text
    - astra.cinematic.camera_pulse
  timeline_tracks:
    - text_effect
    - camera
    - filter
capabilities:
  - filter_graph.node
  - audio_graph.node
  - text_effect.policy
editor:
  visual_model: luau_base_visual_derived
  nodes: true
  inspector: true
  timeline_preview: true
dependencies:
  pesde:
    - name: jsdotlua/luau-regexp
      source: pesde
      version: 1.0.0
package_lock: auto
```

开发期可以通过 pesde 解析外部 Luau 依赖。Package 阶段必须生成 lock/vendor cache，记录版本、hash、license、capability 和来源；CI 和 Release Gate 只接受锁定结果。

多个策略包提供同一 command 或 preset 时，项目 manifest 必须显式绑定 provider。AstraVN 不按加载顺序抢占策略。

标准命令库覆盖 `show`、`hide`、`move`、`camera`、`transition`、`shake`、`movie`、`voice`、`bgm`、`se`、`wait`、`choice` 和 `system_page`。命令扩展必须提供 schema、Editor metadata、IR 输出、skip/auto/replay 规则和 release check。

## Command Registry

命令必须由显式 registry 解析。`story`、`state` 和 `scene` 是结构节点，不是 runtime command。`text`、`choice`、`option`、`jump`、`call`、`return`、`mutate`、`system_page` 和 `wait` 属于 Core command。`background`、`show`、`hide`、`movie`、`voice`、`bgm`、`se`、`timeline`、`task`、`effect` 等 presentation command 必须来自 standard command provider。Extension command 必须来自 project manifest 或 plugin provider 的显式绑定。

计划中的 public 类型包括 `CommandRegistry`、`CommandSchema`、`ChildPolicy`、`CompileOptions` 和 `CompileProfile`。保留 `compile_astra_sources` 作为兼容入口；新增 `compile_astra_sources_with_options` 时再承载 profile、provider binding 和 release checks。

Unknown command 的策略是 fail fast：development profile 可以发 warning 或受控 diagnostic，release profile 必须 blocking，不能静默落入 presentation command。

## Editor Contract

Luau 策略像 UE 的 C++ 基类，Graph/Timeline 像可视派生层。Luau 策略暴露属性、节点、事件和轨道；Editor 允许创作者改参数、事件连接、timeline、fence 和 fallback，不要求展开 Luau 内部算法。

策略包必须提供：

- 参数 schema 和默认值。
- Graph 节点、端口、事件、Inspector 控件。
- Timeline track、preview input/output、fallback。
- source map、diagnostic span、release check。
- performance budget 和采样标签。

Editor 默认按段落/场景级编辑。可视层修改后必须能回写 `.astra`、Luau metadata 或 policy override；不能产生第二套 runtime model。

PIE/Preview 支持 Editor-only Luau refresh。发布 runtime 不支持策略热重载；打包时固定策略、依赖、schema 和 migrator。

## System Stories

System Stories 用项目 YAML 声明入口，内容仍写 `.astra`：

```yaml
system_stories:
  title: system.title
  config: system.config
  gallery: system.gallery
  replay: system.replay
  chart: system.chart
```

官方策略包提供 title、config、gallery、replay、chart、save/load UI、voice replay、backlog 和 localization UI。第三方策略包可以换 UI 和流程，但不能绕过 Core save/read/backlog 状态。

## Localization

文本采用 text key first。`.astra` 引用 key，文本表保存多语言正文、speaker、ruby、voice variant、font fallback 和 layout preview metadata。

```yaml
schema: astra.text_table.v1
locale: zh-Hans
entries:
  prologue.hello:
    speaker: hero
    body: "早上好。"
    ruby: []
    voice: voice.hero.0001
    layout: message.default
```

Editor 提供并排预览。Release Gate 检查缺失 key、voice variant、ruby/layout 不一致和 fallback font 覆盖。

## Reachability

编译器和 Editor 必须提供可达性分析：

- route 覆盖率、死分支、不可达结局。
- 未初始化变量、跨域写入、system 变量非法写入。
- 未读文本、read-state、backlog、voice replay 覆盖。
- system story 入口缺失。
- timeline task 未 join/cancel、Fence 泄漏。

## Release Gate

以下情况阻断发布：

- Luau 写入绕过 `astra.mutate`。
- 策略缺 schema、Editor metadata、migrator、performance budget 或 lock/vendor cache。
- 策略覆盖 Core save/backlog/read-state 语义。
- Luau snapshot 含不可序列化值。
- release profile 中存在未绑定 provider/schema 的 command。
- Graph/Timeline 派生层无法回写 source map。
- source map 缺少 token、attribute 或 macro expansion 级定位，导致 Editor/formatter/LSP 无法回写。
- full playthrough、system stories、save/load、replay hash、backlog/read-state、voice replay、localization preview 任一失败。
- 标准命令 provider 未显式绑定，或 advanced presentation profile 启用后缺少多层 stage、camera、video layer、shader/filter、voice sync、fallback 和性能证据。

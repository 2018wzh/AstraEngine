# AstraVN Module

AstraVN 是原生 VN 垂直模块。它使用 EngineCore 的 Runtime、Script、Media、Asset/VFS 和 Save/Replay。AstraVN Core 持有 VN 权威语义；Luau 策略和插件只负责表现、系统页和演出扩展。

代码布局采用 `Engine/Source/Modules/AstraVN/`。`astra-vn` 只作为 facade crate 存在，负责 `rlib`/Rust ABI `dylib` 输出和兼容 re-export；parser、runtime、policy、presentation、system、save、package、plugin、editor metadata 和 runtime provider 都拆到独立功能 crate。

## Gameplay Runtime Provider

AstraVN 作为 `NativeVnRuntimeProvider` 接入 [Game Runtime Provider](../contracts/game-runtime-provider.md)。Provider 位于 `astra-vn-runtime-provider`，组合 `.astra` compiler、VN Core、Luau policy、presentation/system UI、VN package sections 和 VN release checks，把 dialogue、choice、wait、system page 和 presentation step 转成 Runtime effect、AwaitToken、PresentationCommand、AudioCommand、save section 和 release evidence。

`NativeVnRuntimeProvider` 不作为所有玩法类型的基类。AstraEMU 和后续 AstraRPG 通过各自 runtime provider 与 AstraVN 同级接入；它们不能复用 VN Core 来表达非 VN 的权威状态。

## Rust Dylib Boundary

`astra-vn` 设计为同时产出 `rlib` 和 Rust ABI `dylib` 的 facade。它 re-export AstraVN 子 crate 的 public API，保留现有 `astra_vn::*` 消费路径，但不承载业务实现。这个 dylib 只承诺同 engine version、rustc fingerprint 和 feature fingerprint 下的 Rust-side 动态链接；跨语言或跨编译器稳定边界仍是 `.astra`、package section 和 Stage 1 plugin ABI。

`astra-vn` public API 不传递 Luau VM handle、renderer/audio native handle、Actor 指针或 Editor widget。需要跨插件暴露的能力必须落到 ExtensionRegistry、provider descriptor、package section 和 release report evidence。

## Crate Split

| Crate | 职责 |
| --- | --- |
| `astra-vn-script` | `.astra` source、parser、compiler、`CompiledStory`、source map、debug symbol、route graph、story/variable/command manifest |
| `astra-vn-core` | `VnRuntime`、command cursor、runtime state、choice、call/return、backlog、read-state、voice replay、wait state、system state、replay UI |
| `astra-vn-policy` | Luau sandbox、policy state、mutation/query/trace、policy bundle manifest、source cache、`standard_policy.luau` |
| `astra-vn-presentation` | StageModel、layer/camera/video/audio/timeline/fallback、headless presentation execution、presentation provider manifest |
| `astra-vn-commands` | standard command library、command schema、usage validation、command manifest |
| `astra-vn-system` | system stories、save/config/backlog/gallery/replay/route chart/localization profile |
| `astra-vn-save` | `vn.runtime_state`、`vn.policy_state` save sections、save blob、hash、migration glue |
| `astra-vn-package` | `vn.*` package section plans、profile manifest、commercial baseline、advanced presentation manifest、package evidence |
| `astra-vn-plugin` | VN extension points、extension manifest、provider slot ids |
| `astra-vn-editor` | Graph/Timeline authoring metadata、source round-trip metadata、NativeVN `RuntimeEditorMetadata` |
| `astra-vn-runtime-provider` | `NativeVnRuntimeProvider` composition |
| `astra-vn` | facade、`rlib`/Rust ABI `dylib`、兼容 re-export |

功能 crate 不允许依赖 `astra-vn` facade。需要共享的 DTO 下沉到更底层 crate，不能通过 facade 回引。

## Source

`.astra` 是 canonical story source：

```astra
story main
state prologue #@id story.prologue
  scene room #@id scene.room
    stage:
      background bg_room fade 300 #@id bg.room
      show hero normal at center #@id char.hero.show
    hero: "早上好。" #@id line.hello
    choice "去哪？" #@id choice.where
      "图书馆" -> library #@id choice.library
      "屋顶" -> rooftop #@id choice.rooftop
```

Graph/Timeline 只保存作者视图，必须回写或编译到同一 command id。完整语言、Luau 策略、Editor 可视化和 Release Gate 规则见 [AstraVN Script Spec](astra-vn-script.md)。

## V1 商业 VN 基线

v1 必须覆盖对白、选择、变量、call/return、backlog、auto/skip/read-state、save/load/config、gallery、replay、route chart、voice replay、movie、transition、screen effects、message window、route flags、timed delay blocks 和标准系统页。演出模型见 [AstraVN Presentation Model](astra-vn-presentation-model.md)，命令库见 [AstraVN Standard Command Library](astra-vn-standard-commands.md)，系统 UI 见 [AstraVN System UI Profile](astra-vn-system-ui-profile.md)。

## Luau 扩展

Luau policy 用于 message/choice UI、system stories、presentation preset、timeline preset、复杂演出和插件组合。Luau command 必须声明 schema、snapshot policy、skip/rollback policy、Editor metadata、performance budget 和 release check。Snapshot 只能保存可序列化 scalar/object 值；不可序列化 Luau value 必须变成 blocking diagnostic，不能进入 save/replay。

## Presentation Profiles

商业发布默认检查 `vn.commercial_baseline` 和 `vn.system_ui_profile`。`vn.advanced_presentation` 用于旗舰演出项目，覆盖多层舞台、camera、video layer、shader/filter、voice sync、复杂 text effect、skip/auto/replay 和 fallback；项目 opt-in 后才成为阻断项。

## v1 Release Profile

NativeVN commercial baseline 必须跑通 dialogue、choice、variables、call/return、backlog、auto、skip、read-state、save/load、config、gallery、replay、route chart、voice replay、movie、transition、screen effects、message window、route flags 和 timed delay blocks。缺少任一 system story 入口、Luau policy lock、source map round-trip、command provider binding 或 replay hash 都阻断 VN release profile。

实现细节见 [Game Runtime Provider Blueprint](../implementation/game-runtime-provider.md)、[`.astra` Grammar And IR](../implementation/astra-grammar-ir.md)、[Luau Policy](../implementation/luau-policy.md) 和 [Editor Visual Protocol](../implementation/editor-visual-protocol.md)。

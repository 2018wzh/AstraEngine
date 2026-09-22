# AstraVN Module

AstraVN 是原生 VN 垂直模块。它使用 EngineCore 的 Runtime、Script、Media、Asset/VFS 和 Save/Replay。AstraVN Core 持有 VN 权威语义；Luau 策略和插件只负责表现、系统页和演出扩展。

代码位于 `Engine/Source/Modules/AstraVN/`。`astra-vn` 对外提供 VnSession、构造配置和 typed step 输入输出；会话实现放在内部 session 模块，lib.rs 保持薄入口。其余功能继续按 parser、Core、policy、presentation、system、save 和 package 分工。

## 会话与共享运行层

VnSession 持有 VN 业务状态，通过 EngineSession 使用 RuntimeWorld、Actor/Component、await 和任务作用域。普通剧情推进直接调用 VnRuntime，不再创建转发 FSM 或动态 provider。EngineSession 先校验逻辑步，VN 才提交剧情变更；恢复先验证完整存档，成功后取消旧任务作用域。逻辑仍为 60 Hz，呈现由 Player 独立推进。

旧 runtime-provider crate、Factory 和 FFI 转发已删除。现有包仍校验 `native_vn_descriptor()` 返回的元数据；该函数不提供动态加载能力。其他 gameplay/UI ABI 尚有消费者，见 [重构契约](../contracts/rebuild.md)。

## Crate Split

| Crate | 职责 |
| --- | --- |
| `astra-vn-script` | `.astra` source、parser、compiler、`CompiledStory`、source map、debug symbol、route graph、story/variable/command manifest |
| `astra-vn-core` | `VnRuntime`、command cursor、runtime state、choice、call/return、backlog、read-state、voice replay、wait state、system state、replay UI |
| `astra-vn-policy` | Luau sandbox、policy state、mutation/query/trace、policy bundle manifest、source cache、`standard_policy.luau` |
| `astra-vn-presentation` | StageModel、layer/camera/video/audio/timeline/fallback、headless presentation execution、presentation provider manifest |
| `astra-vn-commands` | standard command library、command schema、usage validation、command manifest |
| `astra-vn-system` | system stories、save/config/backlog/gallery/replay/route chart/localization profile |
| `astra-vn-save` | 局部/reference VN state blob、hash 与 migration glue；产品 provider 的权威 save 是完整 `runtime.world` snapshot |
| `astra-vn-package` | `vn.*` package section plans、profile manifest、commercial baseline、advanced presentation manifest、package evidence |
| `astra-vn-plugin` | VN extension points、extension manifest、provider slot ids |
| `astra-vn-editor` | Graph/Timeline authoring metadata、source round-trip metadata、NativeVN `RuntimeEditorMetadata` |
| `astra-vn` | VnSession、typed 产品入口和 package 元数据 |

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

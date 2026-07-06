# AstraVN Module

AstraVN 是原生 VN 垂直模块。它使用 EngineCore 的 Runtime、Script、Media、Asset 和 Save/Replay。AstraVN Core 持有 VN 权威语义；Luau 策略和插件只负责表现、系统页和演出扩展。

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

Luau policy 用于 message/choice UI、system stories、presentation preset、timeline preset、复杂演出和插件组合。Luau command 必须声明 schema、snapshot policy、skip/rollback policy、Editor metadata、performance budget 和 release check。

## Presentation Profiles

商业发布默认检查 `vn.commercial_baseline` 和 `vn.system_ui_profile`。`vn.advanced_presentation` 用于旗舰演出项目，覆盖多层舞台、camera、video layer、shader/filter、voice sync、复杂 text effect、skip/auto/replay 和 fallback；项目 opt-in 后才成为阻断项。

## v1 Release Profile

NativeVN commercial baseline 必须跑通 dialogue、choice、variables、call/return、backlog、auto、skip、read-state、save/load、config、gallery、replay、route chart、voice replay、movie、transition、screen effects、message window、route flags 和 timed delay blocks。缺少任一 system story 入口、Luau policy lock、source map round-trip、command provider binding 或 replay hash 都阻断 VN release profile。

实现细节见 [`.astra` Grammar And IR](../implementation/astra-grammar-ir.md)、[Luau Policy](../implementation/luau-policy.md) 和 [Editor Visual Protocol](../implementation/editor-visual-protocol.md)。

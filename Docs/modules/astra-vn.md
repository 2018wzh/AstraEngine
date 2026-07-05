# AstraVN Module

AstraVN 是原生 VN 垂直模块。它使用 EngineCore 的 Runtime、Script、Media、Asset 和 Save/Replay，不把 VN 语义放进 Core。

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

Graph/Timeline 只保存作者视图，必须回写或编译到同一 command id。

## V1 商业 VN 基线

v1 必须覆盖对白、选择、变量、call/return、backlog、auto/skip/read-state、save/load/config、voice replay、movie、transition、screen effects、message window、route flags 和 timed delay blocks。

## Lua 扩展

Lua extension 用于自定义 command、route logic、presentation preset 和 EMU patch/decode。Lua command 必须声明 schema、snapshot policy、skip/rollback policy 和 release check。

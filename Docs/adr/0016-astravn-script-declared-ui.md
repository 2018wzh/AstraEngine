# ADR 0016: AstraVN script-declared UI

## Context

`.astra` 已是故事 canonical source，但当前 `system_page` 只声明页面种类和 policy，实际产品仍依赖 `SystemUiModel` 的固定矩形。把 Yakui API直接暴露给脚本会形成第三方 ABI，也会让剧情权威状态、页面流程和 widget transient state 混在一起。

## Decision

`.astra` 增加 backend-neutral UI source role。开发者使用 `ui_view`、`ui_bind` 和 `ui_component` 声明 View、binding、稳定语义节点和 action；Rust 构造只读 schema-bound ViewModel；Luau Controller 处理页面流程并返回可序列化 effect；Yakui 只负责 layout、input、focus、virtualization 和 paint。

Story 与 UI 共享 Lexer、lossless CST、formatter、source map 和 language-service 基础设施，但 lowering 到独立 AST 和 semantic pass。产品编译入口直接迁移为：

```rust
compile_astra_project(...) -> CompiledVnProject
```

`CompiledVnProject` 分别持有 story、UI blueprint、binding、controller、theme 和 source-map hash。Migration 12 完成时删除公开 `compile_astra_sources`、`compile_astra_sources_with_options`、旧 `vn.compiled_story` package reader 和旧 target manifest reader；所有项目重新 Cook，不保留双轨 compatibility API。

Binding 解析顺序固定为 command-specific、system-page、surface `ui_bind`、profile。每一层都必须解析成唯一 binding；不存在隐式默认 provider。

## Consequences

- UI value 仅允许 literal、`$model`、`$item`、`$event`、`$state`、localization key、asset ref 和 theme token，不允许 `eval`、反射或任意函数调用。
- Luau Controller state 只支持 `none` 或 `session`，不进入 save。load 后从 Core/ViewModel 重建。
- UI action 只是 request；save/load、unlock、route jump、cursor 和配置合法性仍由 Core/host authority 决定。
- 开发期 UI/Controller/Theme 热更新失败时立即停止当前 UI session，暂停并保留 Core session，由 host diagnostic overlay 报错；不继续显示最后一次有效 UI。

## Verification

编译器必须覆盖 UI grammar、typed binding、action signature、controller typecheck、source map、format roundtrip、capability binding、旧 API/section/target rejection 和 package recook。产品测试必须从物理输入命中稳定 semantic id，再验证 action、authority result 和输入不穿透。


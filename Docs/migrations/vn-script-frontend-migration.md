# AstraVN Script Frontend Migration

本迁移页记录 `astra-vn-script` baseline 到 lossless frontend 的实施结果。Migration 6 代码迁移状态为 `DONE`；Stage 3 的 script work item 仍须等待同一 package 的 Windows/Web formal Player evidence，不能用 frontend focused tests 代替产品验收。设计依据见 [ADR 0013](../adr/0013-astravn-script-frontend-standardization.md)、[AstraVN Script Spec](../modules/astra-vn-script.md) 和 [`.astra` Grammar And IR](../implementation/astra-grammar-ir.md)。

## 已完成实现

当前 frontend 使用 `logos` 生成 lossless token stream，`chumsky` 负责 command recovery，`rowan` 保存层级 CST，所有 byte span 统一使用 `text-size`。Typed AST 只从 CST wrapper lowering；旧 `parser.rs` line parser 已删除。`compile_astra_sources` 保留为兼容入口，`compile_astra_sources_with_options` 通过 core/standard/extension command registry 阻断未绑定命令。

已落地的边界：

- semantic pass 顺序固定为 symbols、routes、variables、commands、system stories、compiled story。
- `CommandSourceMap` 独立记录 command、keyword、source id、attribute 和 argument span，并带 source-map hash。
- `story_hash` 只覆盖语义内容，不受 trivia、格式、source span、debug symbol 或隐式 command id 影响。
- formatter 写回前必须重新 parse/compile，并比较 semantic hash；写入采用同目录原子替换。
- `ScriptLanguageService` 提供 diagnostics、symbols、definition、references、hover 和 semantic tokens；本轮不实现 stdio LSP。
- package schema 已直接切换为 `astra.vn.compiled_story`；旧 schema或旧布局返回 `ASTRA_VN_RECOOK_REQUIRED`。

## 迁移顺序

| 顺序 | 阶段 | 范围 | 验收 |
| --- | --- | --- | --- |
| 1 | ADR 与 contract 对齐 | 固化 v1 不引入 Cranelift 主线依赖；同步 script spec、contract、grammar IR、status 和 test matrix | `python Tools/check_docs.py` |
| 2 | 依赖引入 | 在 workspace 中引入 `logos`、`rowan`、`text-size`、`chumsky`、`insta` 和 `proptest`；不引入 Cranelift | `cargo metadata --no-deps` |
| 3 | Lexer / Token / Span | 新增 `syntax/token.rs`、`syntax/lexer.rs`、`syntax/span.rs`；覆盖 quote、escape、arrow、source id、indent、duplicate attrs 和 byte range | `cargo test -p astra-vn-script lexer` |
| 4 | Lossless CST / Typed AST | 新增 `syntax/cst.rs`、`syntax/ast.rs` 和 parser recovery；AST adapter 继续输出当前 `CompiledStory` shape | `cargo test -p astra-vn-script --test compiler_runtime` |
| 5 | Semantic Passes | 拆出 `lower::symbols`、`routes`、`variables`、`commands`、`system_stories` 和 `compiled_story` | `cargo test -p astra-vn-script --test compiler_diagnostics` |
| 6 | Command Registry | 新增 `registry/core_commands.rs`、`standard_commands.rs` 和 `extension_commands.rs`；release profile unknown command blocking | `cargo test -p astra-vn-commands --test standard_command_manifest` and `cargo test -p astra-release --test release_report` |
| 7 | Source Map / Debug Symbols | 引入 `SourceSpan`、`CommandSourceMap`、attr/arg span 和 macro expansion frame；Editor metadata 改读新 source map | `cargo test -p astra-vn-editor --test editor_metadata` |
| 8 | Formatter / LSP Adapter | 新增 formatter writer/rules 和 LSP diagnostics/symbols adapter；format 后 semantic hash 稳定 | `cargo test -p astra-vn-script formatter_roundtrip` |
| 9 | Luau authority write 收紧 | `astra.mutate.*` 是唯一 authority write；`astra.var.get` 只读；policy-private cache 不进入 Core state | `cargo test -p astra-vn-policy --test luau_mutation` |
| 10 | 后续语言扩展 | macro、`ExprBytecode`、Cranelift 与 stdio LSP 已移出 Stage 3 完成条件，另行立项 | 不属于 Migration 6 gate |

## Public API 迁移

保留：

```rust
pub fn compile_astra_sources<I>(sources: I) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = AstraSource>;
```

已稳定：

```rust
pub fn parse_astra_source(path: impl Into<String>, text: &str) -> ParsedAstraSource;

pub fn parse_astra_sources<I, S>(sources: I) -> Vec<ParsedAstraSource>
where
    I: IntoIterator<Item = AstraSource>;

pub fn compile_astra_sources_with_options<I>(
    sources: I,
    options: CompileAstraOptions,
) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = AstraSource>;
```

`ParsedAstraSource` 只表示 CST/AST 与 diagnostics，不代表 release-ready IR。Unknown command 可以留在 parse/format/language-service 结果中；只有 registry 已绑定的 command 才能生成 `CompiledStory`。

## Cranelift 边界

本迁移不得把 Cranelift 加进 mainline workspace dependency。未来只有同时满足以下条件才允许新增 optional feature：

- 表达式已 lowering 到 portable `ExprBytecode`。
- Profiling 证明 interpreter 是瓶颈。
- Interpreter/JIT equivalence test 通过。
- Replay hash、package IR、save/load 和 release report 与 JIT 开关无关。
- Web、iOS 和其他 no-JIT target 不受影响。

## 状态回填规则

Migration 6 已关闭，但 `S3-SCRIPT-01` 和 `S3-SCRIPT-02` 保持 `IN_PROGRESS`。只有以下证据都存在时，才能改回 `DONE`：

- Lexer/CST/AST tests 覆盖有效和无效语法。
- Semantic pass tests 证明旧 baseline 行为保持稳定。
- Command registry tests 证明 release unknown command blocking。
- Source map tests 覆盖 token、attribute、argument 和 source-map hash。
- Formatter/LSP tests 证明 round-trip 和 diagnostics mapping。
- Luau mutation tests 证明 authority write 不能绕过 `astra.mutate.*`。
- Stage 3 formal runner 的 Windows/Web 报告来自同一 commit、build fingerprint 和 `.astrapkg`，且没有 direct runtime command、DOM click、JS callback 或自推进 route。
- `python Tools/check_docs.py`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo test --workspace` 通过。

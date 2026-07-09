# AstraVN Script Frontend Migration

本迁移页只写当前 `astra-vn-script` baseline 迁到新设计的路线，不执行迁移。设计依据见 [ADR 0013](../adr/0013-astravn-script-frontend-standardization.md)、[AstraVN Script Spec](../modules/astra-vn-script.md) 和 [`.astra` Grammar And IR](../implementation/astra-grammar-ir.md)。

## 当前 baseline

当前实现已经能把 `.astra` 编译成 `CompiledStory`，并覆盖 story/state/scene/text/choice/option/jump/call/return/mutate/system page、route graph、Story/Variable/Command/System manifest、source map、debug symbols 和 stable hash。该 baseline 仍由 `compile_astra_sources` 兼容入口承载。

需要迁移的短板：

- parser 以 line 为单位，不能保留完整 trivia、comment、blank line 和 token byte range。
- indent 已记录，但还没有 lossless CST 和 block tree 作为 Editor/formatter/LSP 的共同基础。
- `CompileBuilder` 同时承担上下文推进、校验、manifest、route graph 和 IR assemble，pass 边界不够清晰。
- unknown command 仍可能进入 presentation command；release profile 需要显式 command provider binding。
- source map 仍是 line/column/keyword length 粗粒度，不能支持 attribute span、macro expansion stack 和精确 diagnostic。
- Luau policy 需要继续收紧 authority write，避免任何绕过 `astra.mutate.*` 的权威状态写入。

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
| 10 | Expression bytecode | 只有在需要复杂表达式时引入 portable `ExprBytecode` interpreter；Cranelift 只允许未来 optional JIT | `cargo test -p astra-vn-script expr_interpreter` |

## Public API 迁移

保留：

```rust
pub fn compile_astra_sources<I>(sources: I) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = AstraSource>;
```

计划新增：

```rust
pub fn parse_astra_source(source: AstraSource) -> ScriptParseOutput;

pub fn parse_astra_sources<I>(sources: I) -> ScriptParseOutput
where
    I: IntoIterator<Item = AstraSource>;

pub fn compile_astra_sources_with_options<I>(
    sources: I,
    options: CompileOptions,
) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = AstraSource>;
```

`ScriptParseOutput` 只表示 parse/AST 结果和 diagnostics，不代表 release-ready IR。`CompileOptions` 承载 profile、unknown command policy 和 command registry。Release profile 必须 fail fast；development profile 可以保留 warning 级 unknown command diagnostic。

## Cranelift 边界

本迁移不得把 Cranelift 加进 mainline workspace dependency。未来只有同时满足以下条件才允许新增 optional feature：

- 表达式已 lowering 到 portable `ExprBytecode`。
- Profiling 证明 interpreter 是瓶颈。
- Interpreter/JIT equivalence test 通过。
- Replay hash、package IR、save/load 和 release report 与 JIT 开关无关。
- Web、iOS 和其他 no-JIT target 不受影响。

## 状态回填规则

迁移开始后，`S3-SCRIPT-01` 和 `S3-SCRIPT-02` 保持 `REOPENED_SPEC` 或 `IN_PROGRESS`。只有以下证据都存在时，才能改回 `DONE`：

- Lexer/CST/AST tests 覆盖有效和无效语法。
- Semantic pass tests 证明旧 baseline 行为保持稳定。
- Command registry tests 证明 release unknown command blocking。
- Source map tests 覆盖 token、attribute 和 macro expansion 定位。
- Formatter/LSP tests 证明 round-trip 和 diagnostics mapping。
- Luau mutation tests 证明 authority write 不能绕过 `astra.mutate.*`。
- `python Tools/check_docs.py`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo test --workspace` 通过。

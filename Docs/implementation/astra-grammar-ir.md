# `.astra` Grammar And IR

`.astra` 是 AstraVN 的 canonical story source。Migration 6 已完成 lossless frontend：`logos` token stream、`chumsky` recovery、层级 `rowan` CST、CST-backed Typed AST、`text-size` span、固定 semantic passes、Command Registry、semantic/source-map hash、formatter 与 language-service adapter 已落地。Stage 3 script work item 仍等待同 package Windows/Web formal Player evidence。

目标管线：

```text
SourceFile
  -> Lexer
  -> TokenStream
  -> Lossless CST
  -> Typed AST
  -> Semantic Passes
  -> CompiledStory
  -> Manifests / RouteGraph / SourceMap / DebugSymbols / StableHash
  -> Runtime StateMachine
```

Runtime 只消费 `CompiledStory`。Editor 不维护第二套 runtime model；Graph、Timeline 和 Luau metadata 只能回写 source 或 lowering 到同一 IR。

## Current Baseline

当前 `astra-vn-script` 暴露 `compile_astra_sources`，内部以 `ParsedLine` 和 `CompileBuilder` 形成 baseline：

```rust
pub fn compile_astra_sources<I>(sources: I) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = AstraSource>;
```

Baseline 已覆盖 story、state、scene、text、choice、option、jump、call、return、mutate、system page、wait、presentation command、route target、main reachability、text key duplicate、source id duplicate 和基础 source map。它仍不是最终 frontend：它不保留完整 trivia、comment、blank line、token byte range、attribute span、lossless CST、error recovery 或 macro expansion stack。

## Grammar Shape

目标 grammar 仍采用缩进块和具名命令：

```pest
file        = { SOI ~ item* ~ EOI }
item        = _{ story }
story       = { "story" ~ ident ~ command_id? ~ block }
state       = { "state" ~ ident ~ command_id? ~ block }
scene       = { "scene" ~ ident ~ command_id? ~ block }
stage       = { "stage" ~ ":" ~ block }
text        = { "text" ~ kv* ~ command_id? ~ nested_effect? }
choice      = { "choice" ~ kv* ~ command_id? ~ block }
option      = { "option" ~ kv* ~ "->" ~ target ~ command_id? }
timeline    = { "timeline" ~ ident ~ command_id? ~ block }
task        = { "task" ~ ident ~ ":" ~ block }
command     = { "command" ~ path ~ kv* ~ command_id? }
mutate      = { "mutate" ~ var_path ~ op ~ expr ~ kv* ~ command_id? }
system_page = { "system_page" ~ kv* ~ command_id? }
command_id  = { "#@id" ~ ident_path }
kv          = { ident ~ ":" ~ value }
```

Indent handling is a lexer responsibility. The target lexer emits `Indent`、`Dedent`、`Newline`、`TokenSpan` and blocking diagnostics for tabs, odd indent, unclosed quote, missing arrow target, duplicate attrs and empty source id.

## Syntax Model

Planned syntax modules:

```text
syntax/
  token.rs
  lexer.rs
  cst.rs
  ast.rs
  span.rs
  diagnostic.rs
  parse.rs
```

Core token kinds include:

```rust
pub enum SyntaxKind {
    StoryKw,
    StateKw,
    SceneKw,
    TextKw,
    ChoiceKw,
    OptionKw,
    JumpKw,
    CallKw,
    ReturnKw,
    MutateKw,
    SystemPageKw,
    Ident,
    String,
    Colon,
    Arrow,
    SourceIdMarker,
    Newline,
    Indent,
    Dedent,
    Comment,
    Error,
}
```

Lossless CST preserves trivia and original layout. Typed AST extracts author-facing names and spans without resolving assets, variables, policy provider or route reachability.

```rust
pub struct AstFile {
    pub source: SourceFileId,
    pub stories: Vec<AstStory>,
    pub diagnostics: Vec<ScriptDiagnostic>,
}

pub struct AstCommand {
    pub kind: AstCommandKind,
    pub id: Option<StableId>,
    pub args: Vec<AstArg>,
    pub attrs: Vec<AstAttr>,
    pub children: Vec<AstCommand>,
    pub span: SourceSpan,
}
```

## Semantic Passes

Semantic lowering is split into explicit passes:

```text
lower::symbols
lower::routes
lower::variables
lower::commands
lower::system_stories
lower::compiled_story
```

`symbols` collects story/state/scene/command ids and duplicate diagnostics. `routes` resolves jump、call、choice、terminal target and route graph. `variables` validates scopes and builds variable manifest. `commands` resolves Core、standard presentation and extension commands through `CommandRegistry`. `system_stories` validates required pages and policy binding. `compiled_story` assembles manifests, source map, debug symbols and stable hash.

## IR

Current implemented `CompiledStory` fields are:

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

`StageCommand`、typed timeline track/keyframe、token/attribute spans 和 `CommandSourceMap` 已进入当前 IR。`luau_manifest`、macro expansion stack 和 package-bound extension execution 仍是 migration target；Rust schema、package writer、release gate 和测试未共同使用前，不计入实现证据。

IR rules:

- `StableId` order is lexical by source file, byte range, then command id string.
- Every executable command has one source ref today, and a token-level `CommandSourceMap` after migration.
- Text uses key-first localization; inline text extraction must be profile-gated.
- Route graph records every state, jump, call, return and choice edge.
- Policy and command providers resolve from project manifest bindings, not load order.

## Command Registry

`CommandRegistry` 是 Core、standard 和 extension command binding 的编译期真源。Standard command 的 allowed/required 字段由 Rust lowering 函数定义；extension command 则携带完整 descriptor。

```rust
pub struct CommandRegistry {
    commands: BTreeMap<String, CommandProvider>,
}

pub struct ExtensionCommandDescriptor {
    pub command: String,
    pub schema: String,
    pub provider_id: String,
    pub fields: BTreeMap<String, ExtensionFieldContract>,
}
```

Unknown commands are profile-bound. Development profile may warn; release profile blocks if the command has no explicit Core, standard or extension provider binding.

## Source Map

The target `SourceSpan` uses byte range and line/column range:

```rust
pub struct SourceSpan {
    pub file_id: SourceFileId,
    pub byte_start: u32,
    pub byte_end: u32,
    pub line_start: u32,
    pub column_start: u32,
    pub line_end: u32,
    pub column_end: u32,
}
```

`CommandSourceMap` records command span, keyword span, id span, attr spans, arg spans and macro expansion stack. Reports and package sections store hashes, ids and spans only; they must not store source text payload.

## Formatter And LSP

Formatter rules:

- Two-space indent.
- Preserve `#@id` on the same line as the command.
- Preserve comments and blank lines.
- Preserve blank lines between scenes and states.
- Sort key/value args only when the command schema marks them unordered.
- Never rewrite localized text tables from `.astra` formatting.
- Format then parse/compile must keep the same semantic hash.

Initial LSP adapter targets diagnostics, document symbols, hover command schema, go-to-definition for state targets, references for route edges, formatter and semantic tokens.

## Expression Bytecode

Complex `if`、`when` or mutate expressions must first lower to portable bytecode:

```rust
pub enum ExprOp {
    ConstI64(i64),
    ConstBool(bool),
    LoadVar { scope: String, key: String },
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    And,
    Or,
    Not,
}
```

Cranelift can only be a later optional JIT for this bytecode after profiling. Package data, replay hash and release reports must remain portable and independent from JIT availability.

## Tests

Baseline tests remain:

```bash
cargo test -p astra-vn-script --test compiler_runtime
cargo test -p astra-vn-script --test compiler_diagnostics
```

Frontend migration adds tests for lexer spans, CST/AST round-trip, semantic pass equivalence, command registry release blocking, source map spans, formatter stability, LSP diagnostics and expression interpreter equivalence before `S3-SCRIPT-01` or `S3-SCRIPT-02` can return to `DONE`.

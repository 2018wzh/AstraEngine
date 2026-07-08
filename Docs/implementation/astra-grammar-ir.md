# `.astra` Grammar And IR

`.astra` Stage 3 runtime parser 采用轻量 line lexer 与 typed compiler pass。目标语法真源位于 `Engine/Source/Modules/AstraVN/astra-vn-script` 的 serde 类型与 diagnostic 测试；当前单 crate 实现会按迁移计划拆入该 crate。后续 Editor lossless round-trip 可以再引入 CST/parser generator，但不能让 Editor state 混入 runtime IR。

## Grammar Shape

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

Indent handling is a lexer responsibility. 当前 runtime lexer 强制两空格层级、禁止 tab，并为 source map 记录 line/column；后续 CST 层可以显式发出 `Indent`、`Dedent`、`Newline` 和 `TokenSpan`。

## AST

```rust
pub struct AstFile {
    pub stories: Vec<AstStory>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct AstCommand {
    pub id: StableId,
    pub kind: AstCommandKind,
    pub args: Vec<AstArg>,
    pub span: SourceSpan,
}

pub enum AstCommandKind {
    Text,
    Choice,
    Option,
    Stage,
    Timeline,
    Task,
    Command,
    Mutate,
    SystemPage,
}
```

AST preserves author-facing names and spans. It does not resolve assets, variables, policy provider or route reachability.

## IR

```rust
pub struct CompiledStory {
    pub story_manifest: StoryManifest,
    pub system_manifest: SystemStoryManifest,
    pub variable_manifest: VariableManifest,
    pub command_manifest: CommandManifest,
    pub luau_manifest: LuauPolicyManifest,
    pub timeline_ir: TimelineIr,
    pub text_effect_ir: TextEffectIr,
    pub source_map: SourceMap,
    pub debug_symbols: DebugSymbols,
}
```

IR rules:

- `StableId` order is lexical by source file, byte range, then command id string.
- Every executable command has one `CommandSourceRef`.
- Text uses key-first localization; inline primary text is an authoring convenience only if project profile allows extraction.
- Route graph records every state, jump, call, return and choice edge.
- Policy provider is resolved from project manifest bindings, not from load order.

## Diagnostics

Parser diagnostics must include:

```rust
pub struct ScriptDiagnostic {
    pub code: &'static str,
    pub severity: DiagnosticSeverity,
    pub file: SourceFileId,
    pub span: SourceSpan,
    pub message: String,
    pub related: Vec<SourceSpan>,
}
```

Minimum codes: `ASTRA_PARSE_INDENT`, `ASTRA_DUPLICATE_ID`, `ASTRA_UNKNOWN_TARGET`, `ASTRA_UNKNOWN_TEXT_KEY`, `ASTRA_UNBOUND_POLICY`, `ASTRA_ILLEGAL_VARIABLE_SCOPE`.

## Formatter

Formatter rules:

- Two-space indent.
- Preserve `#@id` on the same line as the command.
- Preserve blank lines between scenes and states.
- Sort key/value args only when the command schema marks them unordered.
- Never rewrite localized text tables from `.astra` formatting.

## Tests

```bash
cargo test -p astra-vn --test compiler_runtime
cargo test -p astra-vn --test compiler_diagnostics
```

Expected: CompiledStory IR hash stable, source map/debug symbols stable, grammar and semantic diagnostics carry file/span/code.

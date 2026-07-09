# ADR 0013: AstraVN script frontend standardization

## Context

AstraVN v1 already has the right runtime direction: `.astra` is the canonical story source, `CompiledStory` is the portable runtime IR, and `VnRuntimeState` advances through deterministic Runtime ticks. The current `astra-vn-script` implementation is enough for the existing commercial baseline slice, but it is still a line-oriented parser and compiler builder. That shape is hard to extend into Editor round-trip, formatter, LSP diagnostics, macro source maps, command provider validation, and release conformance.

The tempting alternative is to introduce a machine-code compiler backend such as Cranelift. That does not solve the current problem. Script execution is not the bottleneck being standardized; the unstable boundary is the authoring frontend and the evidence it must produce for Editor, package, release gate, and replay.

## Decision

AstraVN script v1 standardizes the compiler frontend before any native-code backend. The mainline path is:

```text
.astra source
  -> Lexer
  -> TokenStream
  -> Lossless CST
  -> Typed AST
  -> Semantic Passes
  -> CompiledStory
```

The frontend owns token spans, trivia, comments, indentation, attributes, route symbols, semantic diagnostics, source maps, debug symbols, command provider resolution, and formatter/LSP readiness. Runtime still consumes `CompiledStory`; it never executes `.astra` source directly.

Cranelift is not part of the v1 mainline dependency set. It may be considered later only as an optional feature for expression bytecode JIT after profiling proves the portable interpreter is a bottleneck:

```toml
[features]
jit-cranelift = ["dep:cranelift"]
```

Even then, packages save portable bytecode and source maps, not native machine code. Replay hash, save/load, package IR, release reports, Web, iOS, and other no-JIT targets must behave the same with or without the optional JIT.

## Consequences

- Existing `compile_astra_sources` remains the compatibility API.
- Planned parse APIs return typed files plus diagnostics, not a runtime-ready story until semantic lowering succeeds.
- `CompiledStory` remains the runtime boundary. Richer `luau_manifest`, `timeline_ir`, `text_effect_ir`, token spans, macro expansion stacks, and command source maps are migration targets until Rust schema and tests land.
- Release profiles require every command to resolve through an explicit core, standard, or extension command provider. Development profiles may warn on unknown commands; release profiles cannot silently treat unknown keywords as presentation commands.
- Luau authority writes stay behind recorded `astra.mutate.*` APIs. Read-only query helpers and policy-private cache APIs cannot mutate Core authority state.

## Verification

The ADR itself is docs-only:

```bash
python Tools/check_docs.py
```

Implementation migration must add focused tests for lexer/span diagnostics, CST/AST round-trip, semantic pass equivalence, command registry release blocking, source map spans, formatter stability, LSP adapters, Luau mutation bypass blocking, and expression bytecode interpreter equivalence before marking reopened Stage 3 script work as `DONE`.

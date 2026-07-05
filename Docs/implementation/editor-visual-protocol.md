# Editor Visual Protocol

Editor visual state is a derived authoring view. `.astra`, Luau policy metadata and project manifest remain the canonical sources.

## Visual Model

```rust
pub struct VisualNode {
    pub node_id: StableId,
    pub command_id: StableId,
    pub provider: PolicyProviderId,
    pub ports: Vec<VisualPort>,
    pub inspector_schema: SchemaId,
    pub source_ref: SourceRef,
}

pub struct TimelineTrack {
    pub track_id: StableId,
    pub command_id: StableId,
    pub lane: TimelineLane,
    pub clips: Vec<TimelineClip>,
    pub fences: Vec<FenceRef>,
}
```

Visual nodes never execute by themselves. They patch `.astra`, policy override metadata or manifest bindings, then compiler rebuilds IR.

## Round-trip

Edit flow:

```text
source -> parse -> CompiledStory IR -> visual model -> user edit -> source patch -> recompile -> source map identity check
```

If source map identity fails, Editor must show conflict and keep both versions in Review Queue. It cannot silently create a second runtime model.

## Policy Metadata

Luau policy packages expose:

- node name, icon id, port list and parameter schema;
- Inspector controls with type, range, enum and asset filter;
- Timeline track kind and preview renderer;
- diagnostic span mapper;
- performance budget and profiling label.

## Hot Refresh

PIE and Preview can refresh Luau policy. Refresh process:

1. stop policy hooks at a fixed tick boundary;
2. snapshot Core state;
3. reload policy bytecode and metadata;
4. run `compile_preview` checks;
5. resume PIE from the same Runtime state.

Packaged runtime never hot-refreshes policy.

## AI Writes

Project-authorized AI writes produce patch, graph diff, audit event, undo checkpoint and release check. Editor must show all five before applying to canonical source.

## Tests

```bash
cargo test -p astra-editor-bridge graph_timeline_edit
cargo test -p astra-editor-bridge inspector_debugger
cargo test -p astra-editor-bridge release_gate_panel
```

Expected: visual edit round-trips to same command id and failed release check links to source span.

# Luau Policy

AstraVN policy runtime is Luau primary. Legacy engine research can still mention Lua/TJS as historical input, but AstraVN, Editor and plugin policy docs use Luau terminology.

## Runtime

Use `mlua` with Luau support for host binding. Luau runs in a sandbox:

- no filesystem, network, process, clipboard or native call by default;
- no direct access to renderer/audio/platform handles;
- all host capability enters through `astra.*`;
- all authoritative writes go through `astra.mutate`.

## Host API

```luau
astra.command.register(name: string, manifest: CommandManifest, handler: function)
astra.command.filter(name: string, filter: function)
astra.command.emit(name: string, params: table)
astra.command.enqueue(name: string, params: table)

astra.mutate.set_var(scope: string, key: string, value: any)
astra.mutate.presentation(command: table)
astra.mutate.audio(command: table)
astra.mutate.timeline(task: table)
astra.mutate.system_page(event: table)

astra.var.get(scope: string, key: string): any
astra.query.text(key: string, locale: string): table
astra.query.asset(id: string): table
astra.query.backlog(): table
astra.query.savepoint(): table
astra.query.layout(target: string): table
astra.trace.event(kind: string, fields: table)
astra.trace.performance_scope(name: string)
```

Every host function returns either a value or a structured diagnostic table:

```luau
{ ok = false, code = "ASTRA_LUAU_DENIED", message = "...", span = source_span }
```

## Typed Policy

Policy packages must provide Luau types or generated `.d.luau` files. Editor and CI run typecheck before package.

```luau
export type CommandContext = {
  locale: string,
  step: number,
  source_ref: string,
}

export type DialogueParams = {
  key: string,
  speaker: string,
  voice: string?,
}
```

## Policy Manifest

```yaml
schema: astra.policy_bundle.v1
id: astra.policy.standard
version: 0.1.0
runtime: luau
entry: policy/init.luau
types: policy/types.d.luau
commands:
  astra.vn.dialogue:
    params_schema: astra.vn.dialogue.params.v1
    mutation_scope: [backlog, read_state, presentation, audio]
    editor_node: Dialogue
hooks:
  - runtime_start
  - before_command
  - after_command
dependencies:
  pesde:
    - name: jsdotlua/luau-regexp
      version: 0.3.1
lock: astra.policy.lock
```

## Dependencies

Development can resolve pesde packages online. Package builds must write `astra.policy.lock` and vendor cache:

```yaml
schema: astra.policy_lock.v1
packages:
  - name: jsdotlua/luau-regexp
    version: 0.3.1
    source: pesde
    hash: sha256:...
    license: MIT
```

Release Gate uses only lock/vendor cache.

## Snapshot

Allowed runtime snapshot values are nil, boolean, integer, string and object/table values with string or integer keys. Function, thread, userdata, native handle, non-finite number and out-of-range number values block save/replay and package validation.

## Tests

```bash
cargo test -p astra-vn-policy --test luau_sandbox
cargo test -p astra-vn-policy --test luau_mutation
```

Expected: denied capability returns diagnostic, mutation trace records previous value and replay metadata, rollback/playback restores deterministic state, command/query/trace capability calls are serialized, and invalid snapshot/command/trace payloads block.

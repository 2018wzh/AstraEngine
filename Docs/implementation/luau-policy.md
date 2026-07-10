# Luau Policy

Luau 是 AstraEngine 的 policy runtime 方向。当前已实现主体在 AstraVN；Stage 7 需要先把可复用 sandbox、snapshot、trace、manifest、lock/vendor cache 和 diagnostic 抽成 `astra-policy`，再由 AstraVN、AstraRPG 和 AstraEMU host API 绑定各自 namespace。Legacy engine research can still mention Lua/TJS as historical input, but product policy docs use Luau terminology.

## Runtime

Use `mlua` with Luau support for host binding. Luau runs in a sandbox:

- no filesystem, network, process, clipboard or native call by default;
- no direct access to renderer/audio/platform handles;
- all host capability enters through `astra.*`;
- all authoritative writes go through namespace-specific effect/mutation API.

Generic `astra-policy` owns value serialization, capability denial, trace records and package lock/source-cache validation. Product crates own host tables:

- AstraVN uses `astra.command`、`astra.mutate`、`astra.query` and `astra.snapshot`.
- AstraRPG uses `astra.rpg.*` and `astra.rpg.trpg.*`.
- AstraEMU trusted policy host remains Manager/provider scoped and must not expose legacy VM internals.

当前 AstraVN host 每次执行都接收 `PolicyQueryContext` 和 `PolicyExecutionBudget`。Query context 提供 text、asset、backlog、savepoint 与 layout 的确定性快照；缺 key 时返回 blocking diagnostic，不构造合成结果。Query trace 保存 API、参数和 result hash，Replay 使用记录结果。

Execution budget 同时限制 Luau interrupt count、memory bytes、输出记录数量和 snapshot depth。无限循环、内存超限、输出洪泛或过深 snapshot 都会停止执行并返回 diagnostic。

## AstraVN Host API

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

旧的 `astra.var.set` 已禁止作为权威写入入口，调用会返回 `ASTRA_VN_LUAU_AUTHORITY_API`。`astra.mutate.set_var` 只生成可序列化 mutation request/trace；host 必须在 Runtime action 事务中应用权威状态变化。直接修改 Luau table 仍只影响本次策略私有值。

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
cargo test -p astra-policy
cargo test -p astra-vn-policy --test luau_sandbox
cargo test -p astra-vn-policy --test luau_mutation
```

Expected: denied capability returns diagnostic；query reads injected backing state and records result hash；removed authority API and execution budget violations block；mutation trace records previous value and replay metadata；rollback/playback restores deterministic state；invalid snapshot/command/trace payloads block. `cargo test -p astra-policy` is planned until the shared crate exists.

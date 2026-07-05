# Runtime Execution

Runtime determinism comes from a fixed tick pipeline. Async work can run on Tokio, but Runtime state only changes when ordered results enter a tick.

## Tick Order

```text
TickInput
  -> collect PlayerInput
  -> collect AwaitResult sorted by token_id, sequence
  -> apply scheduled RuntimeEvent
  -> run StateMachine guard
  -> run StateMachine action
  -> record MutationLog
  -> emit PresentationCommand / AudioCommand
  -> update AwaitToken queue
  -> compute state/event/presentation hash
  -> TickReport
```

No provider callback may mutate Runtime state directly.

## EventQueue

```rust
pub struct RuntimeEvent {
    pub id: EventId,
    pub source: EventSource,
    pub step: u64,
    pub sequence: u64,
    pub payload: EventPayload,
}
```

Ordering key: `(step, sequence, id)`. When two producers submit at the same logical time, Scheduler assigns sequence before guard/action execution.

## AwaitToken And Fence

```rust
pub struct AwaitToken {
    pub token_id: StableId,
    pub kind: AwaitKind,
    pub requested_at_step: u64,
    pub deterministic_timeout_step: Option<u64>,
    pub replay_policy: AwaitReplayPolicy,
}

pub struct Fence {
    pub fence_id: StableId,
    pub waits_for: Vec<AwaitTokenId>,
    pub fail_policy: FenceFailPolicy,
}
```

Token completion records `AwaitResult`. Replay consumes recorded result and never asks platform/provider again.

## MutationLog

All authoritative state writes use MutationLog:

```rust
pub struct MutationRecord {
    pub mutation_id: StableId,
    pub step: u64,
    pub source_ref: SourceRef,
    pub scope: MutationScope,
    pub before_hash: Hash128,
    pub after_hash: Hash128,
    pub rollback: RollbackRecord,
}
```

Luau policy, VN command, AI committed output and Editor PIE patch all use the same mutation path. Direct writes to Runtime internals are implementation bugs.

## Hash Domains

`TickReport` contains:

- `state_hash`: RuntimeWorld, Actor/Component, StateMachine, Blackboard, VN core state.
- `event_hash`: ordered RuntimeEvent and AwaitResult.
- `presentation_hash`: PresentationCommand, AudioCommand, TextCaptureEvent, policy-visible media state.

Native handles, OS paths, wall-clock time and provider object addresses are excluded.

## Error Handling

Blocking diagnostic stops packaged runtime according to profile. PIE pauses at source span. Recoverable diagnostic continues only when release profile marks the domain as warning.

## Tests

```bash
cargo test -p astra-runtime state_machine_tick
cargo test -p astra-runtime await_token
cargo test -p astra-runtime save_replay
```

Expected: out-of-order async completion still produces matching hashes.

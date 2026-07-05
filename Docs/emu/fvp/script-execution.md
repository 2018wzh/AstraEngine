# FVP Script Execution

FVP VM is a cooperative stack VM. It runs multiple script contexts, but those contexts are engine threads, not OS threads. AstraEMU core should keep that model private and emit deterministic events at fixed tick boundaries.

## Variant and stack frame

rfvp uses these VM values:

| Variant | Meaning |
| --- | --- |
| `Nil` | falsey value and missing value |
| `True` | truthy value |
| `Int(i32)` | integer |
| `Float(f32)` | float |
| `String(String)` | heap string |
| `ConstString(String, u32)` | script-buffer string plus source offset |
| `Table` | integer-keyed VM table |
| `SavedStackInfo` | internal call-frame record |

Every context owns a fixed stack with `MAX_STACK_SIZE = 0x100`. A call pushes `SavedStackInfo`, moves the frame base, jumps to `addr`, and `init_stack` fills argument/local metadata. `ret` and `retv` restore the previous frame. If the restored return address is `usize::MAX`, the context exits.

## Truthiness

The VM treats every non-`Nil` value as true. There is no dedicated false value. `jz` jumps only when the popped value is `Nil`. This matters for `bit_test`, comparisons and `TextPrint` return values.

## Syscall dispatch

Execution of opcode `0x03` is:

1. Read `u16 syscall_id`.
2. Resolve `(args, name)` from the HCB syscall table.
3. Pop `args` values from VM stack.
4. Reverse the popped list to restore call order.
5. Call `VmSyscall::do_syscall(name, args)`.
6. Store the returned `Variant` in the context return register.
7. `push_return` can later push that value back to the stack.

AstraEMU should trace only the stable parts: `thread_id`, `pc`, syscall name, arg count, arg types, result type and yield reason. String bodies and media bytes stay out of reports.

## Thread state

rfvp thread states:

| State bit | Meaning |
| --- | --- |
| `RUNNING` | context may execute opcodes |
| `WAIT` | countdown wait, decremented by frame time |
| `SLEEP` | sleep countdown |
| `TEXT` | blocked on text presentation/user advance |
| `DISSOLVE_WAIT` | blocked until dissolve state completes |

Syscalls do not switch contexts directly. They push `ThreadRequest` values such as `Start`, `Wait`, `Sleep`, `Next`, `TextWait`, `Exit` and `ShouldBreak`. `VmRunner` drains those requests after each opcode dispatch and yields the current context when needed.

## Tick flow

One engine frame does this:

1. Process deferred text resume requests.
2. If game state is halted, skip opcode execution.
3. Capture pending VM snapshot at a safe point.
4. Apply pending load request before executing new opcodes.
5. Advance wait/sleep/dissolve states by `frame_time_ms`.
6. Run each runnable context until it exits or reaches a yield request.
7. Capture post-tick VM snapshot if save requires it.
8. Emit presentation/audio/text/trace outputs for Manager.

In AstraEMU, async IO and media decode must not reorder deterministic state. Any pending external work becomes an `AwaitToken`, and completion enters the core on the next fixed tick boundary.

## Sanitized entry example

The local「樱花萌放」case begins at `entry_point = 223865`. A short sanitized trace shows the VM style without copying story payload:

| PC | Opcode | Operand |
| ---: | --- | --- |
| 223865 | `init_stack` | args `0`, locals `1` |
| 223868 | `push_nil` | |
| 223869 | `pop_global` | global `0` |
| 223872 | `push_global` | global `0` |
| 223875 | `push_true` | |
| 223876 | `set_e` | |
| 223877 | `jz` | addr `223882` |
| 223882 | `push_i8` | `0` |
| 223884 | `pop_global` | global `6` |
| 223947 | `push_i8` | `40` |
| 223949 | `pop_global` | global `1967` |
| 223952 | `push_string` | len `1`, source offset `223954` |
| 223955 | `syscall` | id `109`, `SysProjFolder`, argc `1` |

The example is useful because it exercises header entry, globals, comparison, branch, `ConstString` offset and syscall dispatch without exposing commercial script text.

## Required diagnostics

The FVP core should report:

- HCB header summary and hash prefix.
- Code area bounds and invalid opcode location.
- Syscall not found: `id`, `pc`, `thread_id`.
- Stack overflow/underflow with `pc` and context id.
- Yield reason per context.
- Text wait and resume transitions.
- Snapshot capture/load safe-point result.

Diagnostics must be deterministic and omit payload contents by default.

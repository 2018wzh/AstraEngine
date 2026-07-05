# FVP Implementation Checklist

This checklist is the smallest useful path for AstraEMU FVP. It starts with metadata and deterministic VM evidence before full presentation parity.

## Phase 1: Probe

- Find exactly one root `.hcb`, or report all candidates.
- Parse HCB header, title, NLS, screen mode, global counts and syscall table.
- Scan root `.bin` files and parse pack metadata without reading full payloads.
- Detect loose `movie/*`, `cursor*.ani` and optional subtitle metadata.
- Emit local structured `ProbeContent` report with hash prefixes.

Acceptance:

- `Sakura.hcb` reports `syscall_count = 148`, `custom_syscall_count = 0`, `game_mode = 8`.
- Pack counts match [game-observations.md](game-observations.md).
- Report may contain exact local paths, offsets, sizes and hash prefixes, but no story text or payload bytes.

## Phase 2: Archive And Media Resolver

- Implement `.bin` reader with bounds checks for metadata overflow, missing filename offsets and out-of-range entries.
- Support `folder/name` VFS paths and loose file override.
- Return `Read + Seek` streams for media decoders.
- Classify `hzc1`, `OggS`, `RIFF` and ASF/WMV magic.

Acceptance:

- `bgm/001`, `voice/01000010`, `se/001` and `graph_bg/BG001_000` open through VFS.
- Invalid `../` paths are rejected.
- Reports store pack/entry/offset/size/hash prefix only.

## Phase 3: VM Core

- Decode opcodes `0x00..0x27`.
- Implement `Variant`, fixed stack, globals, call frames and return register.
- Implement syscall dispatch by id/name/argc.
- Implement context table and state bits: running, wait, sleep, text and dissolve wait.
- Emit bounded `StateMachineTrace`.

Acceptance:

- The sanitized entry trace reaches `SysProjFolder` at `pc = 223955`.
- Stack underflow, invalid opcode and missing syscall produce deterministic diagnostics.
- VM state can be snapshotted at a tick boundary.

## Phase 4: Syscall Mapper

- Implement thread, timer, input, flag and utility syscalls first.
- Add graph/prim/text syscalls needed for title and first text wait.
- Add audio load/play/stop/volume with logical channel state.
- Add movie start/stop/state with provider capability reporting.
- Register unknown syscalls as diagnostics, not crashes, until release gate marks them required.

Acceptance:

- Boot reaches title/menu trace without unbounded loops.
- Text wait blocks and resumes through input.
- BGM and one SE path produce `AudioCommand`.
- Missing movie decode reports capability failure without breaking VM state.

## Phase 5: Presentation Bridge

- Convert graph slot changes to texture commands.
- Convert prim tree changes to renderer-neutral primitive commands.
- Convert text slot uploads to text surface commands and optional local text capture.
- Route movies through platform decode first.
- Keep native GPU/audio handles out of IPC.

Acceptance:

- Headless run produces stable state/event/presentation hash for boot and first text wait.
- Renderer run can display title path with no payload stored in report.
- Audio and movie evidence is hash/sample metadata only.

## Phase 6: Save/Load

- Version FVP snapshot sections.
- Store VM globals, contexts, thread manager, graph refs, prim tree, text state, audio logical state and VFS identity.
- Restore only at safe tick boundary.
- Keep older metadata-only saves readable when possible.

Acceptance:

- Save after first text wait, reload, then continue to the same next `StateMachineTrace`.
- Bad snapshot version reports migration error and does not corrupt current core state.

## Release gate

The FVP gate requires:

- `probe_report.json`
- `full_flow_trace.jsonl`
- `state_hashes.json`
- `media_capabilities.json`
- `payload_policy_report.json`

The gate passes only when the report has no payload text, no media bytes and no executable/patch payload. Exact local paths are allowed for local-only reports.

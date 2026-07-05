# FVP Runtime Core Design

AstraEMU FVP runs as an out-of-process compat core. The core owns all FVP-private state. Manager owns windows, platform input, provider selection, overlays, reports and release gate UI.

## Core state

The core contains:

- `FvpProbe`: discovers `.hcb`, `.bin`, loose movie and cursor files.
- `FvpArchiveSet`: pack metadata, VFS lookup and safe stream opening.
- `FvpScript`: HCB parser, NLS, syscall table and opcode decoder.
- `FvpVm`: context array, globals, return register, thread state and deterministic tick.
- `FvpSyscallMapper`: graph/text/audio/movie/input/save/thread syscall implementations.
- `FvpPresentationState`: graph slots, prim tree, text slots, motion/dissolve state and cursor state.
- `FvpAudioState`: channel registry, bus/type volumes, fades and currently loaded media refs.
- `FvpSaveState`: VM snapshot plus FVP-specific graph/text/audio/history state.
- `FvpDiagnostics`: local structured trace ring, coverage counters and failure details.

These types are family-private. EngineCore only sees AstraEMU IPC events and opaque snapshot sections.

## Manager/Core boundary

Core accepts:

- `ProbeContent { root_ref, nls_hint }`
- `LoadCase { case_id, root_ref, hcb_name, nls }`
- `Step { tick_index, frame_time_ms, input_batch }`
- `ApplyInput { tick_index, event }`
- `SaveSnapshot { slot }`
- `LoadSnapshot { slot }`
- `Shutdown`

Core emits:

- `RuntimeEvent`
- `PresentationCommand`
- `AudioCommand`
- `TextCaptureEvent`
- `StateMachineTrace`
- `LegacyVmSnapshotRef`
- `DiagnosticBatch`
- `MediaBlockRef`

The core may read local files through a capability-limited mount issued by Manager. It does not receive arbitrary filesystem authority.

## Determinism

Within one `Step`, VM contexts run in stable numeric order. Each context runs until exit or a yield request. External completion is not applied immediately. It becomes an ordered event consumed at the next `Step`.

Required stable ordering:

1. Input batch for tick.
2. Deferred text resume.
3. Safe-point load request.
4. Wait/sleep/dissolve timer advancement.
5. VM opcode/syscall loop.
6. Presentation/audio/text trace drain.
7. Snapshot capture.

## Save and replay

FVP save state needs both VM-observable state and presentation state:

- HCB identity: hash prefix, title, entry point, sysdesc offset and NLS.
- Globals: non-volatile and volatile values.
- Contexts: stack, pc, frame base, return value, state bits, wait/sleep time and break/exit flags.
- Thread manager: current id and context table.
- Text: text slot buffers, text settings, read bitmap and backlog metadata.
- Graph/motion: loaded graph refs, prim tree, dissolve, snow, motion containers and dirty flags.
- Audio: logical channel bindings, volume, fade and playing state; raw decoder state can be restarted from logical refs unless exact resume is required.
- VFS: pack identity and loose override hash prefixes.

Snapshot sections should be opaque to Manager and versioned under `astraemu.fvp.*`. Runtime replay must not request network or external provider state.

## Release gate inputs

The FVP release gate should accept a local structured case report:

```yaml
schema: astraemu.fvp.case_report.v1
case_id: sakura-moyu-local
hcb:
  bytes: 5002852
  sha256_prefix: 946877dd0ed8fbf3
  syscalls: 148
archives:
  - name: bgm.bin
    entries: 70
    sha256_prefix: 68c621efcf9bef2b
flows:
  boot: pass
  title: pass
  text_wait: pass
  bgm: pass
  movie_probe: pass
payload_policy:
  text_payload: omitted
  media_payload: omitted
```

The report is evidence for local compatibility. It is not a redistributable game sample.

# RFVP Astra Family fork record

`../../ThirdParty/rfvp/` is a source snapshot of the `0.6.0` upstream commit
`304e773387a9920c9db091ec1fd937c717aea949` from
[`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp). The hosted adaptation originated
from the `2018wzh/rfvp` fork at the immutable revision
`f4f64a5bb726c1759350a666a35e0a454b810f61`. The covered source and the complete
MPL-2.0 text are in `../../ThirdParty/rfvp/` and `../../ThirdParty/rfvp/LICENSE`.

The 0.6.0 update incorporates upstream text-wait completion, InputFlash,
dissolve-wait and native global-save fixes. Hosted sessions own script globals per session and never use the process-global
`GLOBAL`. Global/system persistence shares the upstream `GlobalSaveDataV1` type,
bincode codec and RFVG footer. Boot reads `save/rfvp_global.bin` before the VM
runs; explicit Family close writes it through the existing atomic filesystem
adapter. Missing files start a new state; malformed files, mismatched global
counts and I/O failures propagate without overwriting the existing save.
The previous duplicate hosted global capture implementation has been removed.
The unused hosted snapshot/restore and canonical state hash APIs were deleted.
Sixteen previously modified source files now match upstream exactly.
Input, time, timer and motion serialization additions were removed; native slot
snapshots now use the upstream graph and motion layout and load behavior.
The Arc texture ownership adapter remains for shared host rendering resources.

The local changes to the vendored tree are limited to these areas:

- `../../ThirdParty/rfvp/Cargo.toml` removes dependencies on private Astra path crates,
  keeps the hosted feature graph self-contained, records the local `flate2`
  version, and builds the private RFVP core as `rlib` only. The FVP crate in
  the parent directory is the only dynamic plugin boundary.
- `src/host_api/audio.rs`, `src/hosted.rs`,
  `src/audio_player/{bgm_player_host,se_player_host}.rs`, and
  `src/no_std_core.rs` add a typed `is_playing` query and synchronize host
  mixer completion back into RFVP's logical player state. This lets a
  non-looping voice reach EOF without leaving a stale wait predicate.
- `src/subsystem/resources/videoplayer_host.rs` and `src/no_std_core.rs` add a
  typed host stop request and a completion path that clears a host-owned movie
  without emitting a second stop operation.
- `src/soft_render/{framebuffer,renderer}.rs` remove the private
  `astra-byte-source` surface wrapper. Hosted rendering now exchanges a plain
  CPU `Vec<u8>` with the FVP adapter; the adapter performs the final
  Host-owned frame handoff.
- `src/subsystem/resources/text_manager.rs`, `src/font.rs`, and
  `src/no_std_core.rs` keep the four original system-font slots and accept
  host-owned bytes plus a TTC face index. No font is embedded or substituted;
  the Astra adapter resolves exact installed family names with `fontdb`, and
  missing optional faces fail only when selected for rendering.
- `src/app.rs`, `src/rendering/gpu_prim.rs`, and `src/wasm_entry.rs` now match
  the pinned upstream files exactly; formatting-only fork changes were removed.
- `src/subsystem/save_state.rs` shares one capture implementation and the
  upstream region-specific global operations. User-save capture still requires
  the prepared VM snapshot and cannot silently substitute an empty VM.

- `src/no_std_core.rs` preserves native deletion of empty save slots: missing
  files are already deleted, while other filesystem failures propagate. A
  static operation label identifies failing runtime phases without exposing
  game text or filesystem paths.
- Hosted saves prepare the VM snapshot and thumbnail after every coroutine has
  yielded in the frame containing `SaveCreate`, matching the native frame
  boundary, and write that prepared buffer at `SaveWrite`.
  The codec uses the native RFVP calendar/NLS/thumbnail prefix and RFVS state
  footer in `save/rfvp_sNNN.bin`. Invalid headers and missing state fail;
  the earlier RFV9 and text-only readers have been removed. Slot caches are
  updated only after the file write succeeds. VM scheduling waits for host
  capture before another tick can mutate the prepared frame.
  Native slot loading sets each coroutine's return register to `true`, allowing
  the script to rebuild transient state after its saved yield. Queued slot
  refresh, copy, delete, write and load operations finish before scripts resume.
- `src/vm_runner.rs`, `src/subsystem/resources/text_manager.rs`, and
  `src/subsystem/resources/thread_wrapper.rs` cancel text-reveal waiters when
  their coroutine exits or restarts. A delayed completion only releases a
  text wait; it cannot restart an exited coroutine or release another wait.
  Hosted VM failures log the coroutine, instruction address and opcode without
  including script text or arbitrary error payloads.

The AstraEngine `astra-emu-fvp/src/` files are the separate family adapter.
They keep RFVP responsible for game state and use `NativeFileSystem` for
relative game-file access, a direct CPU surface for frame output, the bounded
host audio worker for mixed PCM, and the session-owned WMV playback path for
movie duration, frame timing, and completion. The adapter does not add a
second scene renderer, translation cache, save format, or runtime snapshot
format.

`Docs/emu/fvp/rfvp-fork-audit.md` records the historical v9 audit. It is not
a description of the current independent-host source or its release status.

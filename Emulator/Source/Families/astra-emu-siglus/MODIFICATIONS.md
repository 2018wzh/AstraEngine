# siglus_rs Astra family fork record

`../../../ThirdParty/siglus_rs/` is the git submodule of the full
[`xmoezzz/siglus_rs`](https://github.com/xmoezzz/siglus_rs) repository pinned
to an exact local adaptation commit in the
[`2018wzh/siglus_rs`](https://github.com/2018wzh/siglus_rs) fork. That branch
starts from upstream commit `e762f9f9c1c94cb490fb13f1367fd06f3e1367b4` (2026-09-13)
plus a single adaptation commit
(`c6c4f99ccac855966b726909756afe3200488374`). This local commit consolidates
the original hosted adaptation `2be01ae` and local cancellation/GPU changes
`a9508e6`; both original commits remain available without rewriting their
history. The new commit has not been pushed. The complete MPL-2.0 text stays
in `../../../ThirdParty/siglus_rs/LICENSE-MPL-2.0`; `THIRD_PARTY_NOTICES.md`
records the attribution chain.

## Local changes to the vendored tree

Hosted execution is selected by the `astra-hosted` Cargo feature on
`siglus_scene_vm`. Native renderer and audio paths remain in place; unchanged
platform behavior still requires platform-specific validation.

- `crates/siglus_scene_vm/src/render/mod.rs` adds an offscreen renderer:
  `Renderer::new_offscreen(width, height)` creates the wgpu device without a
  window surface and renders into an owned non-sRGB RGBA8 texture (the same
  D3D9 byte-space blending semantics as the surface path). `render_frame`
  dispatches to a shared `render_frame_to_view` path for both surface and
  offscreen targets, `read_frame_rgba` copies the composed target back to CPU
  RGBA bytes, and target resize recreates the owned texture. The `surface`
  field becomes an `Option`; the desktop overlay shells only gain a missing
  surface error branch.
- `crates/siglus_scene_vm/src/audio/kira_hub.rs` adds a PCM tap audio
  backend gated behind `astra-hosted`: `hosted_tap::install` registers a
  callback and `AudioHub::new` mixes through `TapBackend` (a kira `Backend`
  whose worker pulls mixed frames and pushes interleaved stereo f32 chunks at
  the configured sample rate) whenever a tap is installed. The callback
  blocks on the consumer's bounded queue, pacing the mixer to real time.
  Without an installed tap, `AudioHub::new` takes the unchanged
  `DefaultBackend` path.
- `crates/na_wmv_player/Cargo.toml` bumps the package version from `0.2.0`
  to `0.2.1`: this repository already vendors an unrelated build of the same
  crate name at `Emulator/Source/Families/na_wmv_player`, and two distinct
  path dependencies with identical name and version cannot share a lockfile.
- `crates/siglus_scene_vm/src/runtime/mod.rs`, `src/host.rs`, and
  `src/platform_time.rs` add a deterministic hosted clock behind
  `astra-hosted`: `SiglusHostConfig::deterministic_frame_clock` makes `step`
  drive the frame clock from its `dt_ms` argument, blocking waits measure
  their deadlines against a virtual monotonic clock that advances with each
  frame's elapsed value (`platform_time::hosted_clock`), and the family
  adapter enables it per session. Desktop paths keep the wall clock exactly
  as upstream. Without this, real-time waits (movies, fades, `wait` ms)
  would take their full real-world duration under a headless driver.
- Blanket rustc/Clippy warning overrides from the previous adaptation were
  removed from all 18 affected manifests. Upstream warnings remain visible;
  unrelated lint cleanup is not part of the embedding patch. Formatting-only
  changes in 117 source files were also removed after comparing rustfmt output.

## Family adapter

The local product rebuild adds the independent Family API v3 dynamic root
module and explicit configuration schema. The module rejects a second active
session. Adapter-only game-specific trace output has been removed; bounded
frame events use `tracing`. Failed startup also disables the hosted clock.
Missing `SCREEN_SIZE` retains the engine's documented default, while unreadable,
undecodable or malformed configuration now fails instead of replacing it with
default dimensions. Engine errors expose only an audited operation name;
arbitrary error chains may contain private paths or game text. The first
frame readback now propagates errors instead of retrying every failure.

The local hosted core changes are limited to its existing boundaries:
`TapBackend` receives a cancellation callback and invokes it before joining
the worker, including failed startup and unwinding. Worker panics produce a
stable diagnostic. Offscreen creation rejects CPU and unknown adapters;
the native wgpu pipeline and desktop device backend remain unchanged.
The adapter regression repeats blocked-write shutdown three times through
the real Kira mixer. A separate hardware-only test checks native GPU readback.

`astra-emu-siglus/src/` is the separate family adapter. It owns no engine
state beyond the session: one `SiglusHost` per session on the family session
thread, one offscreen renderer, and one kira tap worker. `open` resolves the
game's `#SCREEN_SIZE` with the same discovery the engine host uses, enables
the hosted virtual clock, boots the engine, and pumps settle frames until the
first composition so the ABI's fixed `FrameInfo` is final. Each `advance`
applies translated family events, pumps one engine frame through
`SiglusHost::step` with the host's elapsed value, and reads the composed
frame back into the session snapshot; a `true` step result (engine-requested
exit or halted proc flow) maps to `FamilyStatus::Finished`. `close` cancels
the host audio queue first, then drops the host so the tap worker joins, and
clears the process-global tap registry. The frame readback is lazy: `advance`
only composes into the offscreen target and `visit_frame` copies to the CPU
on demand, so hosts that skip frame pulls (headless routes) do not pay a
texture-to-buffer copy per advance.

Save data stays in the game directory (`<game>/savedata`), owned entirely by
the engine; the host provides only the game path. Protected retail resources
are decrypted through the user-provided `key.toml` in the game directory.

## Known route blocker (upstream)

The Rewrite+ prologue stalls a few lines into the first message block: the
`sys40_mp20` message proc gates its entire input handling behind a per-message
voice-completion flag (`d[700]`) that never clears in this engine, so the
proc spins on `disp()` and ignores every input shape (single-poll clicks,
split-edge clicks, Enter, Ctrl skip, and the in-game A.Skip toggle). No koe
error is logged and `KoeEngine::is_playing_any()` never reports playback,
so the voice event that should clear the flag never fires. Headless route
completion therefore stops at that point; everything before it (boot, title
flow, opening scene, movie, and the leading dialogue lines) runs and renders
correctly. Fixing it requires upstream work on the msg-block/voice state
machine in `siglus_scene_vm`, not adapter changes.

## Manager diagnostics

The Family adapter installs the shared optional Family API v3 diagnostic bridge before descriptor/probe/open. Existing core tracing and log events reach the Manager sink without adding a core logger or changing native rendering/platform behavior. Unreviewed text and Debug values are redacted with an explicit count.

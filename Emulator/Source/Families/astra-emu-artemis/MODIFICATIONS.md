# art3m1s-core Astra family fork record

`../../../ThirdParty/art3m1s-core/` is the git submodule of the full
[`Alphaly2K/art3m1s-core`](https://github.com/Alphaly2K/art3m1s-core)
repository pinned to the `astra-hosted` branch of the
[`2018wzh/art3m1s-core`](https://github.com/2018wzh/art3m1s-core) fork. That
branch is upstream commit `0c06f37160961c9ff75d4937d5e6bb0500d0bef9` (0.4.0)
plus the adaptation commits below, head `327ec88b30aa605e5e7f9147e8678fea6ff41727`. The complete MPL-2.0 text
stays in `../../../ThirdParty/art3m1s-core/LICENSE`;
`THIRD_PARTY_NOTICES.md` records the attribution chain.

## Local changes to the vendored tree

- `crates/pf8/src/reader.rs` (`38993a9`) normalizes entry-path separators
  when building the in-archive lookup map. Several Artemis titles (for
  example サクラノ詩 / sakuranouta10th.pfs) store entry paths with backslash
  separators while every lookup replaces them with slashes, so all reads on
  such archives failed with "PFS entry disappeared". Both sides now go
  through the same separator rule; lookups stay case-insensitive.
- `Cargo.toml` (`04421af`) moves the Lua backend choice into core features.
  mlua permits exactly one backend per build, and this workspace already
  ships mlua/luau for the AstraVN Luau policy; the adapter therefore builds
  the interpreter on `backend-luau` (upstream's production iOS path) instead
  of `backend-lua51`. Desktop/Android upstream builds keep Lua 5.1 through
  the default feature set; iOS builders now select `backend-luau`
  explicitly.
- `crates/asb-interpreter/src/interpreter.rs` + `src/runtime/mod.rs`
  (`95deb90`) add Lua-flag access for the keyconfig exclick contract:
  `take_global_flag`/`clear_global_key`/`read_global_value` on the
  interpreter, `CoreRuntime::poll_exclick_wake` (consumes `flg.exclick` as
  a decide edge while parked on a `[@]`/bare stop), a host
  `clear_global_key` passthrough for stale button-hover state, and debug
  reads for the wait state, tag queue, and Lua flags.
- `crates/asb-interpreter/Cargo.toml` (`3f73097`) moves the mlua `send`
  feature behind a passthrough default feature. This workspace unifies mlua
  without `send` (Rc-based Lua state owned by one thread), and the runtime
  contract already pins the engine to a single owner thread, so the
  interpreter builds cleanly without it; direct users keep `send` by
  default.
- `src/runtime/mod.rs` (`1ab174f`, `9c43343`, `ddbc9df`) adds three host
  diagnostics/wake entries used by headless drivers; all are inert without an
  explicit host call and do not touch the stable host contract:
  - `debug_wait_state()` reports the current script, line, and wait reason,
    which heads off black-box stalls during route validation.
  - `debug_tag_queue()` reports tags parked behind a wait, distinguishing a
    queued engine jump from a missing handler.
  - `host_decide_wake()` mirrors the documented `setScriptStatus(0)` wake for
    bare `[stop]` waits. Scenario mainloops that park on a bare stop and
    resume from the host decide edge need that edge delivered by the host;
    named stops (video/trans/tween/menu) keep their own release conditions
    and are never woken.

## Family adapter

`astra-emu-artemis/src/` is the separate family adapter. It owns no engine
state beyond the session: one `CoreRuntime` per session on the family session
thread, the process-global `HostEvents` handle, and one software mixer worker.

- `open` finds the base PFS archive, mounts it with the first entry-name
  encoding (UTF-8, Shift_JIS, GB18030) that exposes `system.ini`, points the
  save root at `<game>/savedata`, enables the host-events queue, boots the
  runtime on the platform-default offscreen GPU backend (Vulkan on Windows),
  loads the INI with the `WINDOWS` platform section, and pumps settle frames
  until the first composition so the ABI's fixed `FrameInfo` is final.
- Each `advance` drains the host-events queue (logs go to tracing, media
  commands to the mixer, UI commands to debug tracing), applies translated
  family events, and runs one engine tick through
  `CoreRuntime::advance_and_render_into`, which renders into the offscreen
  target and reads the composed RGBA frame back into the session snapshot. A
  `true` exit request maps to `FamilyStatus::Finished`.
- Input translation feeds Windows virtual-key codes and stage-coordinate
  mouse input. A primary press produces the mouse button edge plus the
  synthetic click edge (`feed_click`), because the script layer's `isDecide`
  polls the click flag or the Enter/Space edges rather than the button edge.
  Hover must precede a press by one tick: Artemis menus bind buttons through
  queued Lua hover handlers.
- The Artemis core has no PCM output; the adapter owns decoding and mixing.
  `audio.rs` streams sources through symphonia (OGG Vorbis, WAV, MP3, FLAC)
  over the core's random-access media source, resamples to 48 kHz stereo,
  applies the Artemis gain/pan/fade and A/B `loop_file` semantics, and pushes
  bounded i16 chunks into the host sink, paced to the wall clock with a small
  lookahead so a fast consumer cannot spin the decoder at full CPU speed.
  Natural end of a non-looping source is reported back through
  `notify_sound_finished`.
- Without a bound FFmpeg provider the adapter cannot decode the core's video
  commands; `video_play` is answered with a delayed
  `notify_video_finished` (default 2 s of engine time,
  `ASTRA_ARTEMIS_VIDEO_FINISH_DELAY_MS`), so opening movies are skipped
  instead of stalling the script while the Lua state migrations scheduled
  around the movie still run. This is a declared degradation, visible in
  tracing.
- `close` cancels the host audio queue first, then joins the mixer worker,
  drops the runtime, and disables the host-events handle.

Save data stays in the game directory (`<game>/savedata`), owned entirely by
the engine; the host provides only the game path.

## Known route blocker (upstream, 終ノ空 remake -2025ver-)

The title flow works end to end (logo movie completion, title menu, START
including the system-voice wait, gamestart transition), but the prologue's
scenario mainloop does not turn text pages: `system/script.asb` parks in a
`Generic` wait at its `click2` position (the `[@]` of the
`scriptMainloop`/`scriptMainAdd` `calllua` pair), and composed frames stay
fixed no matter the decide cadence.

Root cause, traced with the fork's wait-state/tag-queue/Lua-flag diagnostics
and the game's own Lua (`system/adv/keyconfig.lua`, `system/adv/adv.lua`,
`system/adv/button.lua`, `system/adv/mainloop.lua`):

1. Every click during the prologue click wait is dispatched to the
   registered global push handler (`setonpush_calllua`), and
   `global_push_absorbs_default_click` lets that dispatch consume the click
   (`advance=false`), so the engine never treats it as a decide edge.
2. `setonpush_calllua`'s branch chain requires state that does not hold
   while the title's `flg.ui` mode table is still set and `btn.cursor` was
   left stale by the scene change (the button layers are deleted without
   out handlers): it early-returns, `setexclick` never runs, and
   `flg.exclick` stays nil — so the engine-side exclick wake has nothing to
   consume.
3. Force-advancing the wait from the host (or exempting Generic waits from
   push absorption) skips the `calllua` page presentation wholesale: the
   scenario runs to its end within ticks and the game restarts through
   boot, so a raw line skip is not a safe substitute for the missing
   Lua-state re-entry.

Properly supporting this pattern requires upstream interpreter work: deliver
the exclick decide edge into the scenario mainloop's Lua state machine (or
coroutine-style `calllua` yielding) together with the game's mode-transition
contract. Filed upstream as
https://github.com/Alphaly2K/art3m1s-core/issues/3 with the trace evidence.
サクラノ詩 drives its scenario through direct AST chunks and plays fine
through the same adapter. Everything up to the blocker in 終ノ空 remake
(boot, title, movie skip, audio, first scene composition) runs and renders
correctly.

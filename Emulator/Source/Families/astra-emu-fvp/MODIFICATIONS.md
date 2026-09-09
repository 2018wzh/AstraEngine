# RFVP Astra Family fork record

`vendor/rfvp/` is a source snapshot of the `0.5.0` upstream commit
`3b5ea6c96a925c12f95aef8554905e8fecbc77c3` from
[`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp). The snapshot follows the
`2018wzh/rfvp` hosted fork at the immutable revision
`f4f64a5bb726c1759350a666a35e0a454b810f61`. The covered source and the complete
MPL-2.0 text are in `vendor/rfvp/` and `vendor/rfvp/LICENSE`.

The local changes to the vendored tree are limited to these areas:

- `vendor/rfvp/Cargo.toml` removes dependencies on private Astra path crates,
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
- `src/subsystem/resources/text_manager.rs` replaces the upstream Microsoft
  font files with the bundled OFL-licensed Noto Sans SC font while preserving
  RFVP's four system-font slots. The font and its license are recorded next to
  each other under `src/subsystem/resources/fonts/`; its Windows system-font
  scan uses the `WINDIR` environment value and has no fixed drive path.
- `src/app.rs`, `src/rendering/gpu_prim.rs`, `src/script/context.rs`, and
  `src/wasm_entry.rs` contain formatting-only changes from the source import.

The AstraEngine `astra-emu-fvp/src/` files are the separate family adapter.
They keep RFVP responsible for game state and use `NativeFileSystem` for
relative game-file access, a direct CPU surface for frame output, the bounded
host audio worker for mixed PCM, and the session-owned WMV playback path for
movie duration, frame timing, and completion. The adapter does not add a
second scene renderer, translation cache, save format, or runtime snapshot
format.

The pinned RFVP source still contains historical hosted-only code that is not
part of the v9 product contract. Its reachability audit remains in
`Docs/emu/fvp/rfvp-fork-audit.md`; that audit is separate from the source and
license record here.

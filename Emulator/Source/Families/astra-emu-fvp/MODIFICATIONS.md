# RFVP hosted fork record

`astra-emu-fvp` consumes the thin `astra-hosted` fork at the exact revision
recorded in its `Cargo.toml`. Its upstream is
[`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp), fixed to
`3b5ea6c96a925c12f95aef8554905e8fecbc77c3` (0.5.0). Upstream and the forked
source are MPL-2.0; package release binds this record, the MPL-2.0 text, the
source-offer identity, fork revision and family binary identity.

The fork retains RFVP's file layout and platform applications. Its small,
replayable patch stack adds only host-neutral `hosted` capability beside
upstream modules:

- `HostedSession` owns VM globals and transient text per session, accepts one
  bounded input batch, and returns one bounded semantic delta. It never sees
  Astra types, formats, paths, error codes, or platform GPU/audio handles.
- Hosted output carries scene resource/draw mutations, audio resource refs,
  video resource metadata and text print operations. Shipping has no opcode
  trace; Evidence uses a fixed crash ring.
- Snapshot/restore and canonical state identity remain opaque, bounded RFVP
  contracts. Resource reads stay behind the boot-bound RFVP VFS port.

The Astra adapter alone converts that delta to `ScenePacket`, media commands,
local single-use text leases and `PreparedCommit`. It validates the whole
transaction before committing; a malformed delta, resource-policy failure,
restore failure or panic poisons only the affected session. Media bytes and
plaintext never enter public reports.

Fork maintenance starts from the recorded upstream base, keeps each patch
small and module-local, runs upstream hosted checks plus Astra provider tests,
then rebases and records the new exact revision. No Astra product type may be
introduced into the fork.

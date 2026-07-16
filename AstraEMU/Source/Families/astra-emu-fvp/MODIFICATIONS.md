# astra-emu-fvp modification record

Upstream is [`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp), fixed to commit
`657747252eb0d2c5fb4a340695ce6906c2d45133` (tag `0.4.0`). The upstream and
modified covered source are licensed under MPL-2.0.

The AstraEMU derivative keeps upstream-observable behavior for valid FVP input and changes
the product boundary as follows:

- `astra-emu-fvp-rfvp-core` removes ownership of the product window, event loop, filesystem,
  renderer and audio device. It exposes bounded deterministic stepping, serializable snapshots,
  host render/audio/movie journals and VFS callbacks instead.
- Script parsing, opcode dispatch, stack/call-frame behavior, context scheduling and the 148-entry
  release syscall catalog retain the upstream 0.4.0 naming and valid-input behavior. Corrupt input,
  path escape, arithmetic overflow, invalid tick order and exceeded budgets fail deterministically.
- Render, text, audio, movie, input, save/load and read-state operations are translated into ordered
  `LegacyEffect` values. No GPU, audio, VM, actor or native handle crosses the family ABI.
- Family sessions are isolated by stable session id. Panic, malformed output, timeout or failed
  restore poisons only that session and requires controlled shutdown.
- `astra-emu-fvp` adds the `LegacyRuntimeProvider` facade, ABI-safe descriptor, probe/open/step/
  save/restore/shutdown lifecycle, dynamic registration and iOS static registration factory.
- Archive, HZC1/NVSG and media readers add explicit byte/count/depth limits. Unsupported media is
  reported as a compatibility diagnostic rather than guessed or silently accepted.

Git history is the file-level change log. Release packaging must bind this record, the MPL-2.0
license text, the source archive/source-offer identity and the family binary to one package identity.

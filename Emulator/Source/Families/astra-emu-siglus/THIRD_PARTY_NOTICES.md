# Third-party notices

## siglus_rs (`Emulator/ThirdParty/siglus_rs`)

Git submodule of [`xmoezzz/siglus_rs`](https://github.com/xmoezzz/siglus_rs)
pinned by exact gitlink to a local adaptation of the
[`2018wzh/siglus_rs`](https://github.com/2018wzh/siglus_rs) fork: upstream
revision `5f358ef1360b0439e92aed2b749e8f25d8721954` (1.0.0) plus the single
local adaptation `ec20655b32e369c8a6f682a1ecadedddb66c33e6`, for an unofficial
Rust reimplementation of SiglusEngine. The covered source is
redistributed under the MPL-2.0; the complete license text is preserved at
`Emulator/ThirdParty/siglus_rs/LICENSE-MPL-2.0`. The file-level change inventory is
`MODIFICATIONS.md`; both it and the covered source must remain in the source
archive or the valid source offer for a binary distribution.

The complete upstream workspace and vendored components are retained, not
copied into the parent repository. Only dependencies reachable from the
selected Family features are built; the other upstream tools stay source.

Registry dependencies (Kira 0.9, wgpu 0.20, winit 0.31.0-beta.3,
egui 0.28, ab_glyph, encoding_rs, and the transitive closure) and the
upstream-vendored eluna_rs retain their own licenses. Registry packages are
resolved from crates.io and carry their own licenses; the upstream
`Cargo.lock` records the pinned set, and the host workspace lockfile records
the set actually compiled into the family plugin.

The parent `astra-emu-siglus` crate is the independent Family ABI boundary.
The vendored Siglus core is private to the family; the adapter uses the
`astra-hosted` additions described in `MODIFICATIONS.md`.

## siglus-static-key-tool

The SiglusEngine decryption key required by protected retail titles is
supplied by the user through the game directory's `key.toml`; this repository
distributes no key material and no key-extraction tooling.

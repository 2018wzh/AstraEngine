# Third-party notices

## siglus_rs (vendored at `ThirdParty/siglus_rs`)

Git submodule of [`xmoezzz/siglus_rs`](https://github.com/xmoezzz/siglus_rs)
pinned by exact gitlink to a local adaptation of the
[`2018wzh/siglus_rs`](https://github.com/2018wzh/siglus_rs) fork: upstream
revision `e762f9f9c1c94cb490fb13f1367fd06f3e1367b4` (2026-09-13) plus a single
adaptation commit, for an unofficial Rust reimplementation of the
SiglusEngine. The covered source is
redistributed under the MPL-2.0; the complete license text is preserved at
`ThirdParty/siglus_rs/LICENSE-MPL-2.0`. The file-level change inventory is
`MODIFICATIONS.md`; both it and the covered source must remain in the source
archive or the valid source offer for a binary distribution.

The vendored tree keeps the upstream Cargo workspace (`crates/`, `platform/`,
`site/`, `testdata/`) so upstream diffs stay reviewable. Only the crates
reachable from `siglus_scene_vm` are built by this repository
(`siglus_scene_vm`, `siglus_assets`, `shion-xfile`, `shion-render`,
`shion-xscene`, `siglus_omv_decoder`, `na_mpeg2_decoder`, `na_wmv_player`,
`theora-rs`); the decompiler, compiler, and viewer crates remain inert source.

Registry dependencies of the vendored crates (kira, wgpu 0.20, winit 0.30,
egui 0.28, eluna_rs, ab_glyph, encoding_rs, and the transitive closure) are
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

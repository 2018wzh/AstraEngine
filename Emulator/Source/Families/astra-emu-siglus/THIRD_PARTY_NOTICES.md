# Third-party notices

## siglus_rs (`Emulator/ThirdParty/siglus_rs`)

Git submodule of [`xmoezzz/siglus_rs`](https://github.com/xmoezzz/siglus_rs)
pinned by exact gitlink to the published adaptation of the
[`2018wzh/siglus_rs`](https://github.com/2018wzh/siglus_rs) fork: upstream
revision `5f358ef1360b0439e92aed2b749e8f25d8721954` (1.0.0), historical native lifecycle
baseline `13b55b47484faf8dfe03deb6ffd56ca0ec4ba3f2`, and current `astra` revision
[`c0f7e7bbdf51a098288fd2c634d5ba8912786d6f`](https://github.com/2018wzh/siglus_rs/commit/c0f7e7bbdf51a098288fd2c634d5ba8912786d6f)
(ABI 3 port `aed3cbc5a9e5029b2ecbe33c2b114537f94ef5b3` plus the C ABI v3 pin
`2018wzh/AstraEngine@6799938dc33b25bcca4d9a11ddbea1366bf7d93c`, on top of the
previously published `23d082a76dfcb071224056858c49abd53f63cbc3`),
for an unofficial Rust reimplementation of SiglusEngine. Adapter
`a94565cccd4f6d5e0a341e5732c5d98e9de2c71f` and its tree-equivalent Release source
`a67786e80e663e982605e69164ac6bc7d0b148ee` are historical, not the current build source.
The covered source is
redistributed under the MPL-2.0; the complete license text is preserved at
`Emulator/ThirdParty/siglus_rs/LICENSE-MPL-2.0`. The file-level change inventory is
`MODIFICATIONS.md`; both it and the covered source must remain in the source
archive or the valid source offer for a binary distribution.

The complete upstream workspace and vendored components are retained, not
copied into the parent repository. Only dependencies reachable from the
selected native features are built; the other upstream tools stay source.

Kira 0.9.6 is preserved as the complete `vendor/kira` submodule, based on
`dfb08d08d065930e9b257203bf11c6d34742f542` with native worker-ownership fix
`a299b42690094a71885f6ed97dc504c4038da9a7` from `2018wzh/kira`. Its MIT and
Apache-2.0 texts remain in that submodule. The fork pins the CPAL 0.15.3
WASAPI teardown fix `65ac8a6ac1a635cc26bfc5af331295b052044908` from
`2018wzh/cpal`, retaining its Apache-2.0 license and original implementation.
The Windows host build patches that same CPAL 0.15.3 source to
`ecfda841a9140d60bbec1630d47ca91f1a77b8f3` for compatible ALSA dependency
resolution; it retains the WASAPI teardown implementation and native Kira version.

Registry dependencies (wgpu 0.20, winit 0.31.0-beta.3,
egui 0.28, ab_glyph, encoding_rs, and the transitive closure) and the
upstream-vendored eluna_rs retain their own licenses. Registry packages are
resolved from crates.io and carry their own licenses; the upstream
`Cargo.lock` records the pinned set, and the host workspace lockfile records
the set actually compiled into the family plugin.

The parent `astra-emu-siglus` crate is a registration/build/source bridge.
The native C ABI adapter lives in the Siglus fork's `astra/embedded` directory.
Native fixes and embedding changes are separated in `MODIFICATIONS.md`.
The current source includes typed original/CHS resource-pair selection, localized
G01/movie lookup, and an independent managed or explicit save root. These native
capabilities neither execute legacy wrapper patches nor supply missing game fonts.

## siglus-static-key-tool

The SiglusEngine decryption key required by protected retail titles is
supplied by the user through the game directory's `key.toml`; this repository
distributes no key material and no key-extraction tooling.

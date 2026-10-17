# Third-party notices

## siglus_rs (`Emulator/ThirdParty/siglus_rs`)

Git submodule of [`xmoezzz/siglus_rs`](https://github.com/xmoezzz/siglus_rs)
pinned by exact gitlink to a local adaptation of the
[`2018wzh/siglus_rs`](https://github.com/2018wzh/siglus_rs) fork: upstream
revision `5f358ef1360b0439e92aed2b749e8f25d8721954` (1.0.0), native lifecycle
baseline `63ec1f79ec7811f49ad1611668740a09e0d17cc7`, embedded adapter
`49457f433c24c4450c30b215e9518aff68499e29`, and ABI 2 verification fixes
`4779a967d4cb04bb3de252f2bdc20f41dac416bb`, for an unofficial Rust
reimplementation of SiglusEngine. The covered source is
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
The Windows host verification patches that same CPAL 0.15.3 source to
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

## siglus-static-key-tool

The SiglusEngine decryption key required by protected retail titles is
supplied by the user through the game directory's `key.toml`; this repository
distributes no key material and no key-extraction tooling.

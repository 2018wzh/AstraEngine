# Third-party notices

## art3m1s-core (vendored at `ThirdParty/art3m1s-core`)

Git submodule of [`Alphaly2K/art3m1s-core`](https://github.com/Alphaly2K/art3m1s-core)
pinned to the `astra-hosted` branch of the
[`2018wzh/art3m1s-core`](https://github.com/2018wzh/art3m1s-core) fork:
upstream revision `0c06f37160961c9ff75d4937d5e6bb0500d0bef9` (0.4.0) plus a
single adaptation commit. art3m1s-core is an unofficial Rust runtime for the
Artemis visual novel engine. The covered source is redistributed under the
MPL-2.0; the complete license text is preserved at
`ThirdParty/art3m1s-core/LICENSE`. The file-level change inventory is
`MODIFICATIONS.md`; both it and the covered source must remain in the source
archive or the valid source offer for a binary distribution.

The vendored tree keeps the upstream Cargo workspace so upstream diffs stay
reviewable. Only the packages reachable from the `art3m1s-core` root crate
with `default-features = false, features = ["vulkan-backend"]` are built by
this repository (`art3m1s-render` with the Vulkan backend, `art3m1s-media`,
`art3m1s-emote`, `art3m1s-log`, `asb-interpreter` with the vendored Lua 5.1
backend, `pf8`, `pfs-upk-rust`); the RFVP and KRKR adapters, the GL/Metal
backends, the optional FFmpeg session, and the game-probe binaries remain
inert source.

Registry dependencies of the vendored packages (ash, naga, mlua with the
vendored Lua 5.1, ab_glyph, encoding_rs, glam, image, serde_json, and the
transitive closure) are resolved from crates.io and carry their own licenses;
the upstream `Cargo.lock` records the pinned set, and the host workspace
lockfile records the set actually compiled into the family plugin.

The parent `astra-emu-artemis` crate is the independent Family ABI boundary.
The vendored Artemis core is private to the family; the adapter consumes the
public Rust API and the host-events queue described in
`ThirdParty/art3m1s-core/doc/HOST_INTEGRATION.md`.

## pf8

`ThirdParty/art3m1s-core/crates/pf8` is vendored upstream from
[`sakarie9/pfs-rs`](https://github.com/sakarie9/pfs-rs) inside art3m1s-core
and keeps the MIT license at `ThirdParty/art3m1s-core/crates/pf8/LICENSE`;
the single separator-normalization fix in `MODIFICATIONS.md` is applied on
top of it.

## Audio decoding

The family adapter decodes game audio with [`symphonia`](https://crates.io/crates/symphonia)
(MPL-2.0) resolved from crates.io; no codec implementation is vendored in
this repository.

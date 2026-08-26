# RFVP Astra Family ABI v9 fork record

`astra-emu-fvp` consumes the RFVP fork at the exact Git revision recorded in
`Cargo.toml`. The root workspace applies that Git source to the local
`astra-emu-family-api` path crate, so an AstraEngine build has one Family ABI
package identity. A standalone RFVP build resolves the same API from the pinned
AstraEngine ABI commit and does not depend on a local absolute path or an
environment override.

The fork is based on upstream [`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp)
0.5.0 and keeps the upstream MPL-2.0 licensing and source-offer obligations.
The Astra hosted feature is fixed at RFVP revision
`f4f64a5bb726c1759350a666a35e0a454b810f61`.

RFVP directly owns the `Ported + SingleLayer` Family ABI v9 provider:

- it acquires the Host-owned writable surface before rendering and writes the
  software renderer output into that lease without a second framebuffer path;
- it commits `Unchanged`, rectangle, or full damage with the matching surface
  generation;
- it invokes the synchronous Hook at the logical text presentation point and
  retains the original text when the Hook returns a typed failure;
- it performs game-native persistence through the relative-path writable-file
  Host port;
- it produces input, wait, audio, control, and diagnostic DTOs itself.

Family ABI v9 has no snapshot/restore, text lease, scene draw transaction,
runtime content hash, or step-budget contract. The AstraEngine adapter does not
reintroduce those APIs. The pinned fork still contains legacy hosted draw
capture, semantic-delta, snapshot/hash and policy-limit code; that fork-side
residue is recorded as a blocking audit in
`Docs/emu/fvp/rfvp-fork-audit.md` and must be removed in the next fork commit.

The AstraEngine `astra-emu-fvp` crate is only the dynamic-library boundary. It
injects build identity and package metadata, constructs and shuts down the RFVP
provider, contains panics, maps the final error, and emits boundary
observability. It must not translate scenes, compose or copy pixels, infer
texture generations, shape text, cache translations, implement save formats,
or repair RFVP business state.

Fork updates start from the recorded upstream base, keep changes reviewable,
run the RFVP Astra provider tests and the AstraEngine consumer tests, and pin a
new immutable revision. The intended full-damage pixel path remains exactly
RFVP writing the Host lease followed by the Host upload; the current fork does
not yet satisfy the single-path audit because its legacy capture API remains
reachable inside the hosted core.

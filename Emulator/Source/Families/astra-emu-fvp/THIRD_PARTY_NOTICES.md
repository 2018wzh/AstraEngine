# Third-party notices

## RFVP covered source

`../../../ThirdParty/rfvp/` contains the complete fork and MPL-2.0 covered source from
[`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp) `0.6.0` upstream revision
`304e773387a9920c9db091ec1fd937c717aea949`, with the hosted adaptation published in
[`2018wzh/rfvp`](https://github.com/2018wzh/rfvp/tree/codex/local-product-adaptation),
branch `codex/local-product-adaptation`, revision
`73d3b4413c95a3923cce695d98c3d9bf5b08ccf0`.
The complete license text is `../../../ThirdParty/rfvp/LICENSE`. The file-level change
inventory is `MODIFICATIONS.md`; both it and the covered source must remain in
the source archive or the valid source offer for a binary distribution.

The parent `astra-emu-fvp` crate is the dynamic Family ABI boundary. The
RFVP dependency is built from the submodule's `crates/rfvp` with `hosted-gpu`;
the current adapter uses its native GPU renderer, native file system, bounded audio, and
session-owned WMV paths described in `MODIFICATIONS.md`.

## System font binding

The four original RFVP system slots are bound at Family session creation from the host installed MS Gothic, MS Mincho, MS PGothic, and MS PMincho faces. The adapter copies the selected face bytes and TTC index into the private hosted core; this repository distributes no replacement or Microsoft font file. MS Gothic is required at boot; the other slots remain optional until a game requests one.

## WMV decoder dependency

`astra-emu-fvp` uses the in-tree `wmv-decoder` crate at
`../na_wmv_player` for ASF/WMV2/WMA playback. The tracked crate has no
README, license file, copyright header, or upstream URL. Its first appearance
in the repository is the `775ff8243d6d7e82b5839083facbcac6e9230b77` commit;
later history only moves it with the `AstraEMU` to `Emulator` directory rename
and applies local changes.

Several comments refer to FFmpeg routine names and codec standards, but this
tree contains no copied FFmpeg source file or FFmpeg license notice from which
to determine a license. The WMV decoder's provenance and license are therefore
**unknown**. This notice is not a license grant and must not be replaced with
an assumed LGPL, GPL, MIT, or FFmpeg attribution until the implementation's
actual origin is established.

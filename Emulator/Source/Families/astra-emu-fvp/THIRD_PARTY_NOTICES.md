# Third-party notices

## RFVP covered source

`vendor/rfvp/` contains the MPL-2.0 covered source from
[`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp) `0.5.0` upstream revision
`3b5ea6c96a925c12f95aef8554905e8fecbc77c3`, imported from the hosted fork
`2018wzh/rfvp` revision `f4f64a5bb726c1759350a666a35e0a454b810f61`.
The complete license text is `vendor/rfvp/LICENSE`. The file-level change
inventory is `MODIFICATIONS.md`; both it and the covered source must remain in
the source archive or the valid source offer for a binary distribution.

The parent `astra-emu-fvp` crate is the dynamic Family ABI boundary. The
vendored RFVP core is private and is built as `rlib`; the current adapter uses
its hosted core with the direct CPU/native-file-system, bounded audio, and
session-owned WMV paths described in `MODIFICATIONS.md`.

## Noto Sans SC

The RFVP font slot at
`vendor/rfvp/src/subsystem/resources/fonts/NotoSansSC-Variable.ttf` is the
Noto Sans SC variable font from the
[`google/fonts`](https://github.com/google/fonts) source revision
`ec0464b978de222073645d6d3366f3fdf03376d8`. Its source URL is
<https://raw.githubusercontent.com/google/fonts/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf>.
The vendored file has SHA-256
`a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da`.

It is licensed under the SIL Open Font License 1.1. The exact upstream OFL
text is kept beside the font as
`vendor/rfvp/src/subsystem/resources/fonts/OFL.txt`.

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

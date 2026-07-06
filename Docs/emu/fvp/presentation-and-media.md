# FVP Presentation And Media

FVP presentation is syscall-driven. Scripts load resources through VFS, mutate legacy graph/text/prim state, and the renderer/audio/video systems consume that state. AstraEMU should turn those mutations into `PresentationCommand`, `AudioCommand`, `TextCaptureEvent` and media block references.

## Graphics

Common graph syscalls:

| Syscall | Core effect |
| --- | --- |
| `GraphLoad(id, path)` | Load/unload texture slot from VFS |
| `GraphRGB(id, r, g, b)` | Apply tone adjustment to loaded texture |
| `GaijiLoad(code, size, path)` | Register external glyph bitmap |
| `PrimSetSprt(id, texture_id, x, y)` | Initialize sprite primitive |
| `PrimSetText(id, text_id, x, y)` | Bind text slot to primitive |
| `PrimSetTile(id, tile, x, y, w, h)` | Initialize tile primitive |
| `PrimSetAlpha`, `PrimSetBlend`, `PrimSetXY`, `PrimSetWH`, `PrimSetUV`, `PrimSetZ` | Mutate primitive display state |
| `Dissolve`, `DissolveWait` | Drive visual transition and VM wait |

The sample graph packs use `hzc1` payloads. rfvp's `nvsg_pack` describes these as FAVORITE `HZC1 + NVSG` textures and supports `single24`, `single32`, `multi32`, `single8` and `single1`. AstraEMU should decode this inside the FVP core or a family media helper, then pass renderer-neutral surfaces to Manager.

## Text

Text syscalls manage 32 text slots. Important behavior:

- `TextBuff(id, w, h)` creates or clears a text buffer and uploads it to graph slot `4064 + id`.
- `TextPrint(id, content)` writes text, uploads the slot, calls legacy text capture, and may request `TEXT` wait.
- `ConstString` print returns `True` only the first time that script offset is marked as read.
- `TextTest(ConstString)` uses the same read bitmap behavior.
- `TextFormat`, `TextFunction`, `TextSize`, `TextFont`, `TextColor`, `TextSpeed`, `TextSkip` and `TextSuspendChr` update layout and flow control.

`TextCaptureEvent` must contain enough local evidence for replay and backlog, but it should default to local structured text in case reports. When local text capture is enabled, the event still stays inside the user's machine and is not committed to public reports.

## Audio

The sample separates long music, voice and effects:

| VFS family | Sample pack | Entry count | Magic | Typical syscall |
| --- | --- | ---: | --- | --- |
| BGM | `bgm.bin` | 70 | `OggS` | `AudioLoad`, `AudioPlay`, `AudioVol` |
| Voice | `voice.bin` | 14,498 | `OggS` | `SoundLoad`, `SoundPlay` or title-specific mapping |
| SE | `se.bin` | 304 | `RIFF` | `SoundLoad`, `SoundPlay`, `SoundVol` |
| Environment SE | `se_env.bin` | 79 | `RIFF` | `SoundLoad`, `SoundPlay` |
| System SE | `se_sys.bin` | 13 | `RIFF` | menu/system sound syscalls |

rfvp splits BGM-like channels and SE-like channels. BGM channels accept ids `0..3`; sound effects accept ids `0..255`. Fade and crossfade arguments are bounded to `0..300000` ms. Volume uses `0..100`.

## Movie

The sample stores movies as loose files:

| File | Bytes | Magic |
| --- | ---: | --- |
| `movie/01.wmv` | 111,307,131 | ASF header |
| `movie/02.wmv` | 130,225,633 | ASF header |

rfvp's `Movie(path, flag)` treats `flag` by type:

- `flag == Nil`: layer/effect movie, video only.
- `flag != Nil`: modal movie, video + audio, script/scheduler halted while playing.

Resolution tries the original path first for `.wmv`/`.asf` and other native video extensions, then a `.mp4` fallback. For AstraEMU, this fallback is a compatibility probe, not a requirement to transcode or ship converted assets.

## Cursor and input

The sample has loose `cursor1.ani` and `cursor2.ani`. rfvp finds loose `cursor*.ani` files in game roots and maps the numeric suffix to cursor slot. `CursorShow`, `CursorMove` and `CursorChange` mutate window cursor state; input syscalls read per-frame state such as down/up/repeat, cursor position and wheel.

Manager owns the actual OS cursor. Core emits cursor commands and receives input events at tick boundaries.

## Mapping to Astra Runtime outputs

| Legacy state | AstraEMU output |
| --- | --- |
| Graph slot load/unload | `PresentationCommand::TextureLoad` / `TextureRelease` |
| Prim tree mutation | `PresentationCommand::PrimitiveUpdate` |
| Text slot upload | `PresentationCommand::TextSurfaceUpdate` and optional `TextCaptureEvent` |
| Audio channel load/play/stop/vol | `AudioCommand` |
| Movie start/stop/state | `PresentationCommand::VideoStart` / `VideoStop`, `AudioCommand` when modal |
| Thread wait/sleep/text/dissolve | `StateMachineTrace` with yield reason |

No output may carry native GPU/audio handles across provider boundaries. Large media moves through content-addressed media block references.

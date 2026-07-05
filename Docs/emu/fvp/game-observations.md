# Game Observations

This page records local observations from the「樱花萌放」case. It intentionally avoids absolute local paths, story text, screenshots, audio frames and video frames.

## Root layout

Top-level files include one `.hcb`, multiple `.bin` packs, two loose ANI cursors, two loose WMV movies under `movie/`, subtitle files under `subtitle/`, and patch-related files. AstraEMU FVP should treat executable files and installer files as out of scope.

## HCB observations

| Field | Value |
| --- | --- |
| Script | `Sakura.hcb` |
| Bytes | 5,002,852 |
| SHA-256 prefix | `946877dd0ed8fbf3` |
| `sys_desc_offset` | 5,000,782 |
| `entry_point` | 223,865 |
| Non-volatile globals | 1,947 |
| Volatile globals | 1,588 |
| `game_mode` | 8 |
| Runtime size | 1280x720 |
| Title encoding | Shift_JIS |
| Syscall count | 148 |
| Custom syscall count | 0 |

The syscall table starts with audio and control calls, then graph/text/input/thread/save/movie groups. The last syscall id is `147`, mapped to `WindowMode`.

## Archive observations

The case uses pack names that map directly to VFS folders. `bgm/001`, `voice/01000010`, `se/001` and `graph_bg/BG001_000` are enough for smoke tests because they cover Ogg, RIFF and `hzc1` resource types without exposing payload bytes.

| Pack | SHA-256 prefix | Entry count |
| --- | --- | ---: |
| `bgm.bin` | `68c621efcf9bef2b` | 70 |
| `graph.bin` | `fb4b1298468fcb0f` | 1,146 |
| `graph_bg.bin` | `854b895180c6020d` | 375 |
| `graph_bs.bin` | `95a21b6f055faf85` | 594 |
| `graph_sd.bin` | `1fd3a2458664057e` | 57 |
| `graph_vis.bin` | `97a16e548cf2ed38` | 579 |
| `graph_vish.bin` | `e315fa2786a55c28` | 380 |
| `patch.bin` | `04518ff4563ea54d` | 71 |
| `se.bin` | `ce899fff3edf7969` | 304 |
| `se_env.bin` | `0829e51827eeb245` | 79 |
| `se_sys.bin` | `896556c628a38416` | 13 |
| `voice.bin` | `f9b1c7fa81ef7c17` | 14,498 |

## Media observations

- Graph payloads begin with ASCII `hzc1`; rfvp associates this with `HZC1 + NVSG`.
- BGM and voice payloads begin with `OggS`.
- SE payloads begin with `RIFF`.
- `movie/01.wmv` and `movie/02.wmv` begin with ASF GUID bytes and are loose files, not pack entries.
- `cursor1.ani` and `cursor2.ani` are loose cursor files; rfvp uses the numeric suffix as cursor slot.

## VM observations

The entry sequence initializes globals and calls `SysProjFolder` early. A local structured trace should store `pc`, opcode and syscall metadata, not string bodies. A useful smoke trace:

```text
pc=223865 init_stack args=0 locals=1
pc=223869 pop_global global=0
pc=223877 jz addr=223882
pc=223952 push_string len=1 offset=223954
pc=223955 syscall id=109 name=SysProjFolder argc=1
```

## Compatibility concerns

- The sample includes a local汉化 patch layer. AstraEMU should report pack/hash identity and NLS choice but should not document patch install or payload modification steps.
- `patch.bin` is just another VFS pack after legal installation. It is not a reason to make patch semantics part of EngineCore.
- WMV playback should route through platform decode first. If a host cannot decode WMV, the failure belongs in a capability report.
- `filter.dll` and game executables are not runtime inputs for AstraEMU FVP.

# NativeVN Samples

Status: NativeVN Phase 8 full playable demo plus local-data TsuiNoSora conversion sample.

## Overview

NativeVN is the redistributable runtime vertical slice that proves `validate -> cook -> package -> run -> replay -> inspect` without Editor dependencies. TsuiNoSora is a local-data modern port sample: project-local tools convert user-supplied original data into ordinary AstraVN content, then the engine treats it like any other AstraVN sample.

## Key Concepts

- Automated QA uses `AstraGame QA --plan` for suites and keeps `AstraGame launcher --backend sdl --scripted-input` as a lower-level validation path.
- Manual QA checks the same player-visible flows by hand.
- The sample stays NativeVN-only; there is no second playable fixture track.

## Architecture

Sample requirements are specified in [Samples and Test Matrix](../../design/samples-and-test-matrix.md) and [Implementation Coverage](../../design/implementation-coverage.md).

## Programming Guide

Current sample descriptors live in `Samples/*/astra.sample.yaml`. `Samples/NativeVN/Content` contains generated redistributable PNG/OGG fixture media, a `.astra` story script, and a Lua extension schema fixture. `Samples/TsuiNoSora/Tools` contains the project-local converter; generated `Content/` is intentionally untracked.

## API Reference

Descriptors use `schema: astra.sample.v1`; `NativeVN` is the Phase 8 playable demo descriptor and uses redistributable generated fixtures.

## Examples

Run these after packaging:

```powershell
build\Bin\astra.exe package Samples/NativeVN --profile deterministic --json
build\Bin\astra.exe test build\Saved\Packages\NativeVN.astrapkg --plan Samples\NativeVN\Tests\player\nativevn_player.yaml --backend sdl --auto-close --json
build\Bin\astra.exe run build/Saved/Packages/NativeVN.astrapkg --backend sdl --scripted-input Samples/NativeVN/Input/qa/title_walk.yaml --auto-close --json
build\Bin\astra.exe run build/Saved/Packages/NativeVN.astrapkg --backend sdl --scripted-input Samples/NativeVN/Input/qa/system_save_load_systems.yaml --auto-close --json
build\Bin\astra.exe run build/Saved/Packages/NativeVN.astrapkg --backend sdl --scripted-input Samples/NativeVN/Input/qa/menu_reopen.yaml --auto-close --json
```

TsuiNoSora local conversion:

```powershell
python Samples\TsuiNoSora\Tools\convert_tsuinosora.py --source-root <original-path> --output Samples\TsuiNoSora
python Samples\TsuiNoSora\Tools\convert_tsuinosora.py --source-root <original-path> --output Samples\TsuiNoSora --dump-raw-assets --decode-bitd-preview --extract-snds-wav --extract-edim-mp3 --write-assets-patch-draft --accept-route-patch-draft --json
build\Bin\astra.exe validate Samples\TsuiNoSora --strict --json
build\Bin\astra.exe package Samples\TsuiNoSora --shipping --target-platform win64 --json
build\Saved\Shipping\TsuiNoSora\win64\TsuiNoSora.exe
```

Shipping bundles keep the root wrapper executable at the top level and place engine binaries, DLLs, and plugins under `Engine\`.

Optional TsuiNoSora QA validation:

```powershell
build\Bin\astra.exe run build\Saved\Packages\TsuiNoSora.astrapkg --backend sdl --scripted-input Samples\TsuiNoSora\Input\golden.yaml --auto-close --json
build\Bin\astra.exe run build\Saved\Packages\TsuiNoSora.astrapkg --backend sdl --scripted-input Samples\TsuiNoSora\Input\qa\full_route_graph.yaml --auto-close --json
```

For TsuiNoSora release acceptance, first require `Saved/ConversionReports/coverage.json` to report `full_playable_ready: true`, then complete `Saved/ConversionReports/manual_playthrough_checklist.md` against the packaged output and fill `Saved/ConversionReports/manual_signoff.yaml`. Those files are generated from the local conversion and record route coverage, choice-branch coverage, transcript size, save/load/menu checks, and any explicit generated-audio waivers.

Use the sample-local converter's `--check-release-acceptance` flag after signoff to fail fast without reconverting. Use `--require-release-acceptance` when reconverting and checking in one command.

Player automation plans use `schema: astra.test.player_plan.v1`; each case may declare `case_id`, `persona`, `objective`, `steps`, and JSON Pointer `assertions`.
`AstraGame` QA writes `astra.test.player_report.v1`; failed assertions or malformed RuntimeEvent steps fail the command.
Each legacy scripted file may still declare `case_id`, `persona`, `objective`, and `expects`; `AstraGame` launcher writes the result to `playable_vn.player_qa`.

Manual validation checklist:

| Player step | Expected state | Automated case | Evidence field |
| --- | --- | --- | --- |
| Launch packaged demo | SDL3 window opens and package-only run reports passed | all QA cases | `playable_vn.windowed_playable.status` |
| Continue from title | Title continue action is recorded | all QA cases | `playable_vn.system_ui_state.title_continue` |
| Advance dialogue | Message window and nameplate are drawn | all QA cases | `playable_vn.system_ui_state.message_window_drawn` |
| Pick walk branch | Walk route is verified | `title_walk.yaml` | `playable_vn.routes_verified.walk` |
| Open system menu | Menu overlay appears and remains stable | `system_save_load_systems.yaml`, `menu_reopen.yaml` | `playable_vn.system_ui_state.system_menu_opened` |
| Open backlog | Backlog overlay appears | `system_save_load_systems.yaml`, `menu_reopen.yaml` | `playable_vn.system_ui_state.backlog_opened` |
| Open config | Config panel appears and settings apply | `system_save_load_systems.yaml`, `menu_reopen.yaml` | `playable_vn.system_ui_state.config_opened`, `playable_vn.config_state` |
| Save and load slot 1 | Save/load screen appears and slot 1 is used | `system_save_load_systems.yaml`, `menu_reopen.yaml` | `playable_vn.system_ui_state.save_load_screen_opened`, `playable_vn.save_slots` |
| Pick systems branch after load | Systems route is verified | `system_save_load_systems.yaml` | `playable_vn.routes_verified.systems` |
| Hit replay checkpoint | Replay checkpoint is recorded | `system_save_load_systems.yaml` | `playable_vn.replay_checkpoint` |
| Close window | Auto-close succeeds without changing pass status | all QA cases | `playable_vn.player_qa.passed` |

## Troubleshooting

This matrix does not cover gamepad, touch, IME text input, multi-resolution screenshot diff, or long soak play. Add those only when the runtime has real input-device capture and image baseline tooling.

TsuiNoSora conversion fails when required local content cannot be recovered and `Patches/port.json` does not resolve it. Do not commit original or generated commercial content.



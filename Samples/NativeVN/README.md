# NativeVN Player QA Matrix

Status: redistributable playable demo with automated scripted QA and a manual smoke checklist.

## Automated Cases

Run these after packaging:

```powershell
build\Bin\astra.exe package Samples/NativeVN --profile deterministic --json
build\Bin\astra.exe run build/Saved/Packages/NativeVN.astrapkg --windowed-smoke --scripted-input Samples/NativeVN/Input/qa/title_walk.yaml --auto-close --json
build\Bin\astra.exe run build/Saved/Packages/NativeVN.astrapkg --windowed-smoke --scripted-input Samples/NativeVN/Input/qa/system_save_load_systems.yaml --auto-close --json
build\Bin\astra.exe run build/Saved/Packages/NativeVN.astrapkg --windowed-smoke --scripted-input Samples/NativeVN/Input/qa/menu_reopen_smoke.yaml --auto-close --json
```

Each scripted file may declare `case_id`, `persona`, `objective`, and `expects`.
`astra run` writes the result to `playable_vn.player_qa`; failed expectations or malformed YAML fail the command.

## Manual Smoke Checklist

| Player step | Expected state | Automated case | Evidence field |
| --- | --- | --- | --- |
| Launch packaged demo | SDL3 window opens and package-only run reports passed | all QA cases | `playable_vn.windowed_playable.status` |
| Continue from title | Title continue action is recorded | all QA cases | `playable_vn.system_ui_state.title_continue` |
| Advance dialogue | Message window and nameplate are drawn | all QA cases | `playable_vn.system_ui_state.message_window_drawn` |
| Pick walk branch | Walk route is verified | `title_walk.yaml` | `playable_vn.routes_verified.walk` |
| Open system menu | Menu overlay appears and remains stable | `system_save_load_systems.yaml`, `menu_reopen_smoke.yaml` | `playable_vn.system_ui_state.system_menu_opened` |
| Open backlog | Backlog overlay appears | `system_save_load_systems.yaml`, `menu_reopen_smoke.yaml` | `playable_vn.system_ui_state.backlog_opened` |
| Open config | Config panel appears and settings apply | `system_save_load_systems.yaml`, `menu_reopen_smoke.yaml` | `playable_vn.system_ui_state.config_opened`, `playable_vn.config_state` |
| Save and load slot 1 | Save/load screen appears and slot 1 is used | `system_save_load_systems.yaml`, `menu_reopen_smoke.yaml` | `playable_vn.system_ui_state.save_load_screen_opened`, `playable_vn.save_slots` |
| Pick systems branch after load | Systems route is verified | `system_save_load_systems.yaml` | `playable_vn.routes_verified.systems` |
| Hit replay checkpoint | Replay checkpoint is recorded | `system_save_load_systems.yaml` | `playable_vn.replay_checkpoint` |
| Close window | Auto-close succeeds without changing pass status | all QA cases | `playable_vn.player_qa.passed` |

## Current Limits

This matrix does not cover gamepad, touch, IME text input, multi-resolution screenshot diff, or long soak play. Add those only when the runtime has real input-device capture and image baseline tooling.

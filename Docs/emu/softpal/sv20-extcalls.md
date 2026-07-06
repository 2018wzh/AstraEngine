# Sv20 Extcalls

`Sv20` primary opcode `23` 是 extcall。参数为：

```text
raw = (category << 16) | index
dst_slot_raw = next u32
```

VM 在 dispatch 前保存 `dst_slot_raw`。handler 返回 value 时，VM 按 destination slot 写回。handler 返回 wait 时，也要先写入指定 value，再把 wait request 交给 tick scheduler。

## Signature table

`sena-rs::pal-script::extsig` 是 evidence-oriented table。每条签名包括：

| field | 用途 |
| --- | --- |
| `category` / `index` | extcall 编号 |
| `canonical_name` | decompiler 和 trace 的稳定名字 |
| `pop_count` | runtime 清理 argument stack 的数量 |
| `params` | pop-order 参数说明 |
| `return_kind` | void、integer、bool、handle、status 等 |
| `side_effects` | sprite、audio、text、save、history、file/profile 等副作用 |
| `implementation_status` | `Verified`、`Partial`、`Blocked`、`Stub`、`Unknown` 等 |
| `evidence` | trace、disassembly、writeup、Game/PAL sqlite 引用 |

AstraEMU 不需要把整张表暴露给 Manager，但 core report 要能按 category/index 给出 name、pop_count、status 和 concern。

## Category map

| category | 主要职责 | 代表 extcall |
| ---: | --- | --- |
| `2` | ADV text、text window、voice-linked text state | `text`、`text_w`、`text_set_base` |
| `3` | sprite / face / sprite text | `sp_set`、`sp_set_ex`、`sp_set_pos`、`face_set` |
| `4` | BGM | `bgm_play`、`bgm_stop`、`bgm_set_volume` |
| `5` | SE | `se_load`、`se_play`、`se_wait` |
| `6` | select/menu choice | `select_init`、`select_set`、`select_commit` |
| `7` | waits and synchronization | `wait`、`wait_click`、`wait_sync_*` |
| `8` | button groups | `btn_set`、`btn_show`、`btn_get_push` |
| `9` | system/font/input/window helpers | `set_font_size`、`skip_set`、`auto_set` |
| `10` | save/load UI and save data | `save`、`load`、`savepoint` |
| `11` | movie/MSP | `movie_play`、`msp_wait` |
| `12` | system buttons | `system_btn_set` |
| `13` | voice/BGV | `voice_play`、`voice_wait`、`voice_set_volume` |
| `14` | history/backlog | `history_set`、`history_scroll` |
| `15` | misc system overlay/debug | `system_window_overlay_set` |
| `16` | transition/effect stop | `effect_stop_skip` |
| `17` | action/tween scheduler | `action_timeline_*` |
| `18` | file/string/profile/app helpers | `file_exist`、`openfile`、`getprivateprofileint` |
| `20` | random | `random` |
| `21` | script thread helpers | `create_thread`、`exit_thread` |
| `22` | run / sub-script flow | `run`、`run_no_wait`、`run_stack` |
| `23` | message queue helpers | `create_message`、`get_message` |

## Stack example

合成示例：

```text
push imm(0)        ; slot
push imm(102)      ; FILE.DAT resource id
push imm(0)        ; x
push imm(0)        ; y
pack_args imm(4)
extcall ext_0003_0002.sp_set, dst_slot[0]
```

runtime handler 读取 `pop[0]` 到 `pop[3]`，不要按显示顺序猜。decompiler 可以把它渲染成 `sp_set(slot, resource, x, y)`，但 VM 的真实输入仍是 pop-order vector。

## Return writeback

`dst_slot_raw == 0` 时，多数 handler 仍要清理参数，但不写回。非 0 时按 operand/slot 规则写入。AstraEMU trace 要记录：

```text
pc=0x00001234 ext=0002:000A name=text_get_time dst=0x40000006 return=12345
```

## Unknown extcall policy

Unknown extcall 不是 fatal 的唯一选择。AstraEMU 可以按 signature 或 observed arity 清理 stack，返回 0，并把 route 标成 `DONE_WITH_CONCERNS` 的 evidence。但如果该 extcall 有 presentation、audio、save 或 control-flow side effect，release gate 不能把对应场景算作通过。

## Runtime/Event mapping

extcall handler 不直接触碰 EngineCore 对象。推荐映射：

| side effect | AstraEMU output |
| --- | --- |
| text state | `TextCaptureEvent` + `PresentationCommand::TextWindow` |
| sprite state | `PresentationCommand::Sprite*` |
| audio | `AudioCommand::Load/Play/Stop/Volume` |
| wait | serializable `AwaitToken` |
| save/load | `LegacySnapshotEnvelope` + diagnostics |
| file/profile read | capability-checked local read result |

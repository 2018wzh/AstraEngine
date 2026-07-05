# FVP Script Format

FVP 主脚本通常是 `.hcb`。rfvp 的 parser 没有 magic check，文件开头直接是 `sys_desc_offset`，code area 从 offset `0x00000004` 开始，到 `sys_desc_offset` 之前结束。

## Header

```text
offset 0x00:
  u32 sys_desc_offset

offset sys_desc_offset:
  u32 entry_point
  u16 non_volatile_global_count
  u16 volatile_global_count
  u8  game_mode
  u8  game_mode_reserved
  u8  title_len
  u8[title_len] game_title_c_string
  u16 syscall_count
  SyscallDesc[syscall_count]
  u16 custom_syscall_count

SyscallDesc:
  u8 args
  u8 name_len
  u8[name_len] name_c_string
```

Strings are encoded by case NLS. rfvp accepts `sjis`/`shift_jis`, `gbk`/`gb18030`, and `utf8`. HCB C-strings are emitted as a one-byte length that includes the trailing NUL.

## Game mode

rfvp maps `game_mode` to fixed virtual resolution. The sample has `game_mode = 8`, so the runtime resolution is `1280x720`.

| Mode | Size |
| ---: | --- |
| 0 | 640x480 |
| 1 | 800x600 |
| 2 | 1024x768 |
| 3 | 1280x960 |
| 4 | 1600x1200 |
| 5 | 640x480 |
| 6 | 1024x576 |
| 7 | 1024x640 |
| 8 | 1280x720 |
| 9 | 1280x800 |
| 10 | 1440x810 |
| 11 | 1440x900 |
| 12 | 1680x945 |
| 13 | 1680x1050 |
| 14 | 1920x1080 |
| 15 | 1920x1200 |

## Opcode set

| Byte | Mnemonic | Operand |
| ---: | --- | --- |
| `0x00` | `nop` | none |
| `0x01` | `init_stack` | `i8 args`, `i8 locals` |
| `0x02` | `call` | `u32 addr` |
| `0x03` | `syscall` | `u16 syscall_id` |
| `0x04` | `ret` | none |
| `0x05` | `retv` | none |
| `0x06` | `jmp` | `u32 addr` |
| `0x07` | `jz` | `u32 addr` |
| `0x08` | `push_nil` | none |
| `0x09` | `push_true` | none |
| `0x0a` | `push_i32` | `i32 value` |
| `0x0b` | `push_i16` | `i16 value` |
| `0x0c` | `push_i8` | `i8 value` |
| `0x0d` | `push_f32` | `f32 value` |
| `0x0e` | `push_string` | `u8 len`, bytes |
| `0x0f` | `push_global` | `u16 global_id` |
| `0x10` | `push_stack` | `i8 local_offset` |
| `0x11` | `push_global_table` | `u16 global_id` |
| `0x12` | `push_local_table` | `i8 local_offset` |
| `0x13` | `push_top` | none |
| `0x14` | `push_return` | none |
| `0x15` | `pop_global` | `u16 global_id` |
| `0x16` | `pop_stack` | `i8 local_offset` |
| `0x17` | `pop_global_table` | `u16 global_id` |
| `0x18` | `pop_local_table` | `i8 local_offset` |
| `0x19` | `neg` | none |
| `0x1a` | `add` | none |
| `0x1b` | `sub` | none |
| `0x1c` | `mul` | none |
| `0x1d` | `div` | none |
| `0x1e` | `mod` | none |
| `0x1f` | `bit_test` | none |
| `0x20` | `and` | none |
| `0x21` | `or` | none |
| `0x22` | `set_e` | none |
| `0x23` | `set_ne` | none |
| `0x24` | `set_g` | none |
| `0x25` | `set_ge` | none |
| `0x26` | `set_l` | none |
| `0x27` | `set_le` | none |

In rfvp code the enum names for `0x25` and `0x27` are historically misnamed, but the displayed mnemonic and behavior are `set_ge` and `set_le`.

## Sample header

Observed from the local「樱花萌放」case:

| Field | Value |
| --- | --- |
| File | `Sakura.hcb` |
| Bytes | 5,002,852 |
| SHA-256 prefix | `946877dd0ed8fbf3` |
| `sys_desc_offset` | 5,000,782 |
| `entry_point` | 223,865 |
| `non_volatile_global_count` | 1,947 |
| `volatile_global_count` | 1,588 |
| `game_mode` | 8 |
| Title | `さくら、もゆ。 -as the Night's, Reincarnation-` |
| Title encoding | Shift_JIS |
| `syscall_count` | 148 |
| `custom_syscall_count` | 0 |

First syscall descriptors:

| ID | Args | Name |
| ---: | ---: | --- |
| 0 | 2 | `AudioLoad` |
| 1 | 2 | `AudioPlay` |
| 2 | 1 | `AudioSilentOn` |
| 3 | 1 | `AudioState` |
| 4 | 2 | `AudioStop` |
| 5 | 2 | `AudioType` |
| 6 | 3 | `AudioVol` |
| 7 | 5 | `ColorSet` |
| 8 | 1 | `ControlMask` |
| 9 | 0 | `ControlPulse` |
| 10 | 1 | `CursorChange` |
| 11 | 3 | `CursorMove` |

Last descriptors include `TimerSuspend`, `TitleMenu`, `V3DMotion`, `V3DMotionPause`, `V3DMotionStop`, `V3DMotionTest`, `V3DSet` and `WindowMode`.

## Format policy

AstraEMU may expose a structured HCB metadata report, but the report must not include full `push_string` payloads. Store string length, source offset, encoding and hash prefix. Full text stays inside the compat core and only enters `TextCaptureEvent` when the user has explicitly enabled local text capture for a legally owned copy.

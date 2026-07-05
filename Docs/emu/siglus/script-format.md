# Siglus Script Format

Siglus `.ss` 是 binary scene stream，不是文本脚本。AstraEMU 不需要恢复作者源文件；它需要按原 VM 的 operand 消费顺序执行 bytecode，并能输出可定位的本地结构化 trace。

## 基本类型

| form | 参考值 | 含义 |
| --- | ---: | --- |
| `FM_VOID` | 0 | 无返回 |
| `FM_INT` | 10 | 32-bit integer |
| `FM_INTLIST` | 11 | integer list |
| `FM_STR` | 20 | string |
| `FM_STRLIST` | 21 | string list |
| `FM_LIST` | -1 | nested arg/list marker |

Form 和 element code 不是 AstraEngine public API。它们只在 Siglus core 内解释。

## Opcode 表

`CD_*` opcode 是单字节，后接 little-endian operand。

| opcode | 名称 | operand |
| ---: | --- | --- |
| `0x00` | `CD_NONE` | none |
| `0x01` | `CD_NL` | `i32 line` |
| `0x02` | `CD_PUSH` | `i32 form, i32 value_or_string_id` |
| `0x03` | `CD_POP` | `i32 form` |
| `0x04` | `CD_COPY` | `i32 form` |
| `0x05` | `CD_PROPERTY` | none |
| `0x06` | `CD_COPY_ELM` | none |
| `0x07` | `CD_DEC_PROP` | `i32 form, i32 prop_id` |
| `0x08` | `CD_ELM_POINT` | none |
| `0x09` | `CD_ARG` | none |
| `0x10` | `CD_GOTO` | `i32 label` |
| `0x11` | `CD_GOTO_TRUE` | `i32 label` |
| `0x12` | `CD_GOTO_FALSE` | `i32 label` |
| `0x13` | `CD_GOSUB` | `i32 label, ArgFormList` |
| `0x14` | `CD_GOSUBSTR` | `i32 label, ArgFormList` |
| `0x15` | `CD_RETURN` | `ArgFormList` |
| `0x16` | `CD_EOF` | none |
| `0x20` | `CD_ASSIGN` | `i32 left_form, i32 right_form, i32 arg_list_id` |
| `0x21` | `CD_OPERATE_1` | `i32 form, u8 op` |
| `0x22` | `CD_OPERATE_2` | `i32 left_form, i32 right_form, u8 op` |
| `0x30` | `CD_COMMAND` | `i32 arg_list_id, ArgFormList, i32 named_count, i32[named_count], i32 ret_form` |
| `0x31` | `CD_TEXT` | `i32 read_flag` |
| `0x32` | `CD_NAME` | none |
| `0x33` | `CD_SEL_BLOCK_START` | none |
| `0x34` | `CD_SEL_BLOCK_END` | none |

`ArgFormList` 的读取顺序是：

```text
i32 count
repeat count:
  i32 form
  if form == FM_LIST:
    nested ArgFormList
reverse decoded arg form list
```

这个 reverse 行为来自 `siglus_ss_decompiler` 和 VM cross-check。少这一步会让 `CD_COMMAND` 的参数形态错位。

## 运算符

`CD_OPERATE_1`/`CD_OPERATE_2` 使用单字节 op：

| op | 含义 |
| ---: | --- |
| `0x01` | `+` |
| `0x02` | `-` |
| `0x03` | `*` |
| `0x04` | `/` |
| `0x05` | `%` |
| `0x10` | `==` |
| `0x11` | `!=` |
| `0x12` | `>` |
| `0x13` | `>=` |
| `0x14` | `<` |
| `0x15` | `<=` |
| `0x20` | `&&` |
| `0x21` | `||` |
| `0x30` | `~` |
| `0x31` | `&` |
| `0x32` | `|` |
| `0x33` | `^` |
| `0x34` | `<<` |
| `0x35` | `>>` |
| `0x36` | unsigned right shift |

整数使用 wrapping 算术。除零和取模零在参考 VM 中返回 0。字符串比较先 lowercase，再比较。

## Element chain

Element 是 `i32` code 链，不是对象指针。常见特殊值：

| 值 | 含义 |
| ---: | --- |
| `-1` | `[]` array/index marker |
| `-2` | `__set` |
| `-3` | `__trans` |
| `-4` | `current` |
| `-5` | `up` |

User property 和 user command 通过 owner byte 区分。参考实现使用 `owner=127` 表示 user prop，`owner=126` 表示 user command。AstraEMU trace 可以渲染已知 symbol，但 IPC 只传数字链和本地结构化 label。

## 本地结构化示例

不要输出原脚本文本。可输出：

```json
{
  "scene": "seen01000",
  "line": 128,
  "pc": "0x00000420",
  "op": "CD_COMMAND",
  "command": "global.open_wait",
  "args_hash": "sha256:..."
}
```

`CD_TEXT` 可输出 text length、read flag 和 hash；不可输出完整台词。

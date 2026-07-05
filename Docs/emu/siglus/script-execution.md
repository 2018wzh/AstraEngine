# Siglus Script Execution

Siglus VM 是 stack-based interpreter，加上 form/element dispatch。AstraEMU core 要复刻 VM 行为；Manager 只看到稳定事件。

## 执行状态

最小 VM state：

```text
SceneVm
  SceneStream { pc, current scene chunk }
  int_stack: Vec<i32>
  str_stack: Vec<String>
  element_points: Vec<usize>
  call_stack: Vec<CallFrame>
  gosub_return_stack: Vec<(pc, ret_form)>
  user_props: map
  scene_stack: Vec<SceneExecFrame>
  current_scene_no, current_scene_name, current_line_no
  save_point
  selection savepoint stack
  CommandContext
```

`CALL_SCRATCH_SIZE` 在参考 VM 中是 32，匹配原 C++ call local scratch list。

## 指令步进

每步读取一个 opcode，并按 [script-format.md](script-format.md) 的 operand 顺序消费。`CD_NL` 更新 `current_line_no`。`CD_PUSH` 根据 form 把 int 或 string 压入对应栈；string operand 是字符串表 ID。

控制流：

| 指令 | 行为 |
| --- | --- |
| `CD_GOTO` | 跳到 `label_list[label]` |
| `CD_GOTO_TRUE` | pop int，非零跳转 |
| `CD_GOTO_FALSE` | pop int，零跳转 |
| `CD_GOSUB`/`CD_GOSUBSTR` | 保存 return pc 和 ret form，进入 label |
| `CD_RETURN` | 弹出 call/gosub frame，按 ret form 回写 |
| `CD_EOF` | scene 结束或返回上一 scene frame |

跨 scene 调用走 scene stack。`jump(scene, z)` 替换当前 scene；`farcall(scene, z)` 压入当前 frame 后进入目标 scene。

## Command dispatch

`CD_COMMAND` 先在 VM 中解析参数，再进入 `CommandContext` 的 numeric form dispatch：

```text
external form handler
  -> built-in forms::dispatch_form
  -> unknown recorder
```

External handler 只能作为 Siglus core 内部扩展点，用于 game-specific form。不跨 ABI 暴露原生对象所有权、窗口句柄、GPU handle 或 Actor 指针。

返回值按 `ret_form` 写回 VM 栈：

| ret form | VM 写回 |
| --- | --- |
| `FM_VOID` | 清空 return stack slot，无值 |
| `FM_INT`/label | push int |
| `FM_STR` | push string |
| `FM_LIST` 或其他 form | push element chain |

## Wait 和 proc boundary

Siglus 原引擎有 cooperative proc model。AstraEMU 需要把会挂起的命令转成显式 wait state，不让 host task completion order 改变脚本状态。

Core 内 wait kind：

| kind | 触发 |
| --- | --- |
| `MessageWait` | `wait_msg`、page 等文本等待 |
| `KeyWait` | 输入等待 |
| `TimeWait` | `timewait` |
| `MovieWait` | movie playback wait |
| `WipeWait` | transition wait |
| `AudioWait` | voice/BGM/SE wait |
| `Selection` | choice/select button |
| `SystemModal` | system message box/config/save/load |

每个 wait 结束后，结果在固定 tick 边界进入 VM。Manager 只发送 input event，不直接改 VM 栈。

## Savepoint

参考 VM 区分 live runtime 和 local save snapshot。`savepoint` 捕获 scene、line、pc、int/str stack、element points、call stack 和 local runtime stream。正常/快速存档应读取最近 savepoint，而不是当前菜单 UI 的 live 状态。

AstraEMU snapshot 分两层：

| 层 | 内容 |
| --- | --- |
| `LegacyVmSnapshotRef` | core 私有 VM、scene stream、栈、user props、local save |
| Manager report | snapshot hash、scene name、line、资源引用、feature coverage |

Report 不包含完整 local stream。

## Unknown 处理

未知 form/opcode 不应让 Manager 崩溃。Core 记录：

```text
scene, line, pc, opcode/form id, arg shape hash, current proc kind
```

若未知点阻断玩家流程，scenario 状态为 `BLOCKED`。若可跳过但 presentation 不完整，状态为 `DONE_WITH_CONCERNS`。

# Artemis Script Execution

## Boot

Artemis 启动先读 `system.ini`。平台段决定 stage resolution、字符集、根脚本和保存路径。

本地样本都使用：

| 样本 | Windows stage | `CHARSET` | `BOOT` |
| --- | --- | --- | --- |
| サクラノ詩10th | 1920x1080 | UTF-8 | `system/first.iet` |
| 终之空Remake2025 | 1600x1200 | UTF-8 | `system/first.iet` |

移动和 WASM 段都使用 1280x720 baseline。AstraEMU 的 Windows compat path 先实现 `WINDOWS` 段；其他平台段只进入 diagnostics，不能影响 desktop core 的 deterministic replay。

## Boot Script Flow

两个样本的 `system/first.iet` 都是短启动脚本，tag 名包括：

```text
debug, lua, calllua, wt, return, stop, loading, if, load, lydel
```

サクラノ詩10th 的 boot 链里可见 `system_initlua`、`init_patch`、`system_starting`、`font_cache`、`langsel_startup`、`brand_logo`、`title_cache`、`title_init`。终之空Remake2025 还出现 `system_loadinglua`、`system_dataloading`、`system_initialize`。

这些函数名可以进入 trace；Lua 函数体和商业脚本正文不能进入报告。

## Execution Model

Artemis family plugin 的最小状态：

```text
ArtemisCoreState
  resolver
  current_script
  instruction_pointer
  call_stack
  variable_store(g/t/s)
  lua_state
  tag_filter
  queued_tags
  layer_tree_current
  layer_tree_future
  audio_state
  wait_state
  save_state
```

每个 tick 只做固定顺序：

1. 如果有 `wait_state`，检查时间、输入、SE、video、scenario tween 是否满足。
2. 执行当前 script row 或 queued tag。
3. tag 输出 `PresentationCommand`、`AudioCommand`、`TextCaptureEvent` 或 state mutation。
4. 如果 tag 进入等待，生成可序列化 `AwaitToken`，在固定 tick 边界恢复。
5. 写入 ordered trace batch。

Tokio 可用于 IO 和解码，但 deterministic state 不依赖 task completion order。异步加载结果必须通过 `AwaitToken` 回到 tick 队列。

## Tag Dispatch

tag dispatch 先过 Lua tag filter：

```text
raw tag -> normalize params -> setTagFilter hook -> built-in tag or skip -> output event
```

返回值规则：

- filter 返回 `0`：继续执行原 tag。
- filter 返回 `1`：跳过原 tag。
- filter 抛错：记录 diagnostics，按 policy 停止脚本或进入 recoverable error。

`e:tag` 是立即执行，`e:enqueueTag` 是当前 Lua 函数退出后顺序执行。`enqueueTag` 队列和脚本栈独立，遇到 `jump` 时必须按 Artemis 规则清理或标记不直观顺序，不能让队列跨越脚本跳转污染下一段状态。

## Wait Points

官方 `wait` tag 支持：

| 条件 | 参数 | AstraEMU 映射 |
| --- | --- | --- |
| 时间 | `time` | `AwaitToken::Timer(ms)` |
| 输入 | `input=1/2` | `AwaitToken::InputOrSkipPolicy` |
| scenario tween | `scenario=1/2` | `AwaitToken::PresentationFence` |
| SE | `se=<id>` | `AwaitToken::AudioFence(id)` |
| video layer | `video=<id>` | `AwaitToken::VideoFence(id)` |

`trans`、`video`、`loading`、surface load 和 async tag 也必须落到同一类 await/fence 机制。

## Variables

| 前缀 | 生命周期 |
| --- | --- |
| 无前缀 | 普通脚本变量，随 save/load 进入 snapshot |
| `g.` | 全局变量，不随 load 回滚 |
| `t.` | 临时变量，不保存；load、reset、退出时删除 |
| `s.` | 系统变量，由引擎读写；部分只读但脚本仍可能覆盖 |

`s.bgmvol`、`s.sevol`、`s.videovol`、`s.automodewait` 这类可写变量直接映射到 family state。`s.engineversion`、`s.datapath`、`s.savepath` 这类只读变量由 family plugin 提供，写入时记录 warning。

## Save/Load

Artemis save snapshot 至少包含：

- 当前脚本路径、row index、call stack、queued tag policy。
- 变量 store，按普通、`g.`、`t.`、`s.` 生命周期区分。
- layer tree、message layer、backlog metadata、already-read state。
- audio state、loop marker、voice replay link metadata。
- Lua serializable state。官方资料提到 Pluto；AstraEMU 可先只支持可序列化白名单对象，其他 Lua state 进入 `DONE_WITH_CONCERNS`。

## Trace

Trace 不写 payload，只写可复现事件：

```text
tick, script_id, row_index, tag_name, param_keys, wait_reason, output_event_kind
```

正文、完整参数值、图片名、音频名可以按 release policy hash 或保留 basename；默认 report 使用 basename 和 hash prefix。

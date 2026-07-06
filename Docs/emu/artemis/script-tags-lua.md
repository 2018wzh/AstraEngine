# Artemis Script Tags and Lua

## 官方互操作

Artemis 有两条 Lua 入口：

| 入口 | 行为 |
| --- | --- |
| `[lua]...[/lua]` | 文件加载时执行 Lua block；通常用于初始化全局变量和函数 |
| `[calllua function="name"]` | 执行指定 Lua 函数；第一个参数是 engine object，惯例名为 `e` |

所有文件共享同一个 Lua 环境。AstraEMU 不能按脚本文件创建隔离 Lua VM，否则 `system/first.iet` 里加载的系统函数会找不到后续 ADV 模块注册的全局函数。

## Engine Object

基础 `engine` object allowlist：

| 函数 | 行为 | AstraEMU 状态 |
| --- | --- | --- |
| `e:tag{...}` | 立即执行 tag；所有参数必须是字符串 | DONE |
| `e:enqueueTag{...}` | 当前 Lua 函数退出后排队执行 tag | DONE_WITH_CONCERNS，需验证 jump/call 清队列细节 |
| `e:setTagFilter(table)` | 设置 tag filter，覆盖或拦截 tag | DONE |
| `e:getScriptStatus()` | 查询脚本状态 | DONE_WITH_CONCERNS，状态枚举需从样本补齐 |
| `e:getScriptWaitReason()` | 查询等待原因 | DONE_WITH_CONCERNS，需和 `AwaitToken` 对齐 |
| `e:setScriptStatus()` | 修改脚本状态 | BLOCKED，必须先确认状态值和恢复点 |
| `e:getScriptStack()` / `e:setScriptStack()` | 读写脚本栈 | BLOCKED，涉及 save/load 和 call/return 精确语义 |
| `e:debug()` | 输出日志 | DONE |
| `e:var()` | 访问变量 | DONE_WITH_CONCERNS，需覆盖 `g/t/s` 生命周期 |
| `e:file()` / `e:isFileExists()` | 文件能力 | DONE_WITH_CONCERNS，仅允许 read-only resolver |

官方文档还包含 `callShellExecute`、HTTP、clipboard、native call 等能力。AstraEMU 默认不开放这些能力；需要时只输出 diagnostics 或走用户授权 capability。

## Tag Filter

`setTagFilter` 的参数是 table，key 是 tag 名，value 是 Lua function。filter 收到 `e` 和 `param`。例如内建剧情文本最终可看成 `print` tag，因此过滤 `print` 可以改变文本处理。

AstraEMU 的 filter dispatch：

```text
if tag_filter[tag.name] exists:
    result = lua_call(filter, e, params_as_string_table)
    if result == 1: skip built-in tag
    if result == 0: run built-in tag
else:
    run built-in tag
```

参数表必须保留 Artemis 的字符串语义。即使参数看起来是数字，也先作为字符串传入 Lua，只有 built-in tag 执行时再按 tag schema 转型。

## `e:tag`

官方示例把：

```text
[lyc id="0" file="bg"]
```

写成：

```lua
e:tag{"lyc", id="0", file="bg"}
```

如果 Lua 变量是数字，调用方必须 `tostring`。AstraEMU 可以在 diagnostics 中指出非字符串参数，但不能悄悄改变原始 Lua 语义。

## `e:enqueueTag`

`enqueueTag` 用于安排会让脚本循环退出的 tag，例如等待、过渡、加载。执行时机是当前 Lua 函数返回之后。多个 tag 按入队顺序执行。

需要特别处理：

- queue 与脚本栈独立。
- `e:tag` 里执行 `call`、`return`、macro 时，执行顺序可能不直观。
- `e:tag` 里执行 `jump` 时，官方文档说明会丢弃 stack 并清空 queue。

这些规则要写进 trace，否则 replay 很难解释输入后脚本位置为何变化。

## `calllua`

`calllua` tag 只有必需参数 `function`。本地 boot 脚本中出现的函数名包括：

```text
system_initlua, init_patch, system_starting, font_cache, brand_logo, title_cache, title_init
```

这些名称可作为 diagnostics 和 coverage 维度。函数体、项目私有逻辑和完整参数不能写入通用文档或 report。

## Sandbox

AstraEMU 的 Lua 5.4 sandbox 规则：

- 默认无文件、网络、进程、clipboard、native call。
- `engine` object 是唯一 host capability 入口。
- PFS/loose resolver 只读，且只能访问 case root 内文件。
- 时间、随机数、输入和 async 完成结果必须可 replay；需要通过 core-provided deterministic source。
- 任何 Runtime AI 或外部 provider 结果必须在 IntentValidator 后固化进 save/replay，回放不重新请求 provider。

## Minimum Tests

- `[lua]` block 在文件加载时注册函数，而不是运行到 tag 时才注册。
- `calllua` 能收到 engine object。
- `setTagFilter` 返回 0 时原 tag 继续，返回 1 时原 tag 被跳过。
- `e:tag` 立即影响 layer/audio/text state。
- `e:enqueueTag` 在 Lua 返回后执行，并在 `jump` 后清队列。

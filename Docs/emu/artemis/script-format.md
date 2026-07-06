# Artemis Script Format

## 文件族

| 扩展 | 观察 | 用途 |
| --- | --- | --- |
| `.iet` | 文本脚本，官方语法是 tag + 文本 | boot、系统流程、staff 等短流程 |
| `.ast` | 样本中是 `ast = { ... }` 形式的 Lua table-like script | 正文场景和已经结构化的 tag 行 |
| `.asb` | `ASB\0` 开头，含 tag/string table | 系统流程的 binary script |
| `.lua` | Lua 源码 | ADV 系统、按钮、配置、文件、key event 等 |
| `.ipt` | `ipt = { ... }` table | 图像 atlas、动画或按钮切片 metadata |
| `.tbl` | `init = { ... }` table | 系统配置、UI 表、语言表等 |
| `.sli` | `# Sound Loop Information` | OGG loop label 或 sample position metadata |

`.ast/.ipt/.tbl` 在样本中都能用 Lua table 方式初步扫描，但不能直接等同于可执行 Lua。AstraEMU 应先建只读 parser，输出结构化 command row 和 diagnostics，再决定哪些表可交给 Lua 语法子集解析。

## `.iet` text script

官方语法规则：

- 默认 Shift_JIS；`system.ini` 的 `CHARSET=UTF-8` 可切到 UTF-8。
- CRLF 和 LF 都可接受。
- 非 `[` 与 `]` 包围的内容是剧情文本。
- `[tag key=value key="value"]` 是 command tag。
- 行首空格和 tab 会被忽略。
- 变量可由 `var` 创建，`$` 开头的参数值按表达式求值。
- `g.` 是全局变量，`t.` 是临时变量，`s.` 是系统变量。

Parser 必需输出：

```text
ScriptText(text, source_span)
Tag(name, params, source_span)
Label(name, source_span)
LuaBlock(source_span, body_hash)
```

`LuaBlock` 不保存完整 body 到 case report，只保存 hash、行数和导出的函数名。

## `.ast` table script

两个样本的 `.ast` payload 解密后通常以 `ast = {` 开头。观察到的 row 以 tag 名为第一项，例如系统或场景中出现：

```text
text, rt2, ruby, bg, fg, vo, se, bgm, msg, msgoff, extrans, quake, cacheclear
```

这些名称不是官方 tag 文档的完整集合，也包含项目自定义 ADV macro。AstraEMU 不能把它们提前提升成 EngineCore API；它们只属于 Artemis family 的 tag/macro layer。

`.ast` parser 必需输出：

```text
AstTable {
  rows: [
    { tag: "bg", params: {...}, source_index: 0 },
    { tag: "text", params: {...}, source_index: 1 }
  ],
  labels: {...},
  raw_hash: ...
}
```

正文参数进入 `TextCaptureEvent` 前必须本地结构化处理；报告只保留长度、语言、ruby 标记数量和 tag 名。

## `.asb` binary script

样本中的系统 ASB 以 `41 53 42 00` 开头，也就是 `ASB\0`。可见字符串包括 `select`、`calllua`、`jump`、`return`、`save`、`wait`、`stop` 等。当前已确认的是：

- 它是 Artemis 系统脚本的一种 binary/compiled 表达。
- 它携带 tag 名、参数名和 label/string table。
- `system/script.asb`、`system/ui.asb`、`system/save.asb` 都在两个样本中出现。

ASB opcode 和 table layout 需要 synthetic fixture 或更完整的公开资料确认后才能写成 contract。现阶段 adapter 只要求 probe 能识别 magic、提取可打印 tag/string metadata，并把执行支持标记为 `DONE_WITH_CONCERNS`。

## `.lua`

官方 `lua` tag 文档说明所有文件共享一个 Lua 环境。样本系统脚本使用 `system/adv/*.lua` 组织 ADV 逻辑，例如 `adv.lua`、`button.lua`、`conf.lua`、`fileio.lua`、`fsave.lua`、`keyevent.lua`、`mainloop.lua`。

AstraEMU 只暴露 `engine` object 的 capability allowlist。默认禁止文件、网络、系统调用和外部进程；需要文件或 HTTP 行为的原始 tag 只能映射成诊断事件或用户显式允许的 host capability。

## 辅助表

| 文件 | 结构 | 用法 |
| --- | --- | --- |
| `.ipt` | `ipt = { key = "x,y,w,h" }` 或动画 table | UI hit region、atlas crop、动画 metadata |
| `.tbl` | `init = { ... }` | game/system config；可进入 diagnostics，但不要复制商业文本 |
| `.sli` | text header + `Label { Position=...; Name=... }` | BGM/voice loop marker；转换为 AudioCommand loop metadata |

## 验收

- 读取 `CHARSET` 后能正确解析 UTF-8 和 Shift_JIS `.iet`。
- 能从 `.iet` 输出 tag 序列、Lua block hash、label 和 source span。
- 能从 `.ast` 输出 tag 频次和 command row，不保存正文。
- 能识别 `.asb` magic 并提取 tag/string metadata。
- 能解析 `.sli` 的 loop position，保留 sample position，不读音频 payload。

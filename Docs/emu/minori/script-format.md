# Minori Script Format

## 当前 parser contract

当前样本的 `.sc` 不是二进制 framed bytecode，而是 CP932、CRLF 行式源码。89 个文件共 33728 行，其中 33695 行为 `.command operands`，另有 28 行注释和 5 行空白；旧的 `opcode:u16 + operand_size:u16` 假设已被样本否定并删除。

parser 逐字节保留原始行与换行，同时记录 command ordinal、source span、ASCII command token 和 raw operand。原程序 tokenizer 把每个 ASCII space 和 tab 当作一个 operand 分隔符，连续分隔符产生空的 positional operand；逗号、单双引号都属于普通字节，不承担 quoting 或 escaping。当前 parser 已按这一 contract 修正。只有显式 `ScOpcodeCatalog` 中登记且经结构验证的 command 才获得控制流语义；未知 command 或未确认的 `select` 分支语义保留为 `Unknown`。

已实现的阻断项包括非法 CP932、重复 command spec、重复 label、无效本地 jump target、operand schema mismatch 和 lossless source invariant。`encode_sc(parse_sc(bytes))` 是 round-trip 门禁。完整正文与近源码 disassembly 只写 ignored 私有研究目录。

## 全包 census

| 分类 | 结果 | 证据边界 |
| --- | ---: | --- |
| `.sc` 文件 | 89 | 当前本地样本全包 |
| command | 33695 | 29 个已知 token，未知 token 为 0 |
| `message` | 18319 | 已确认四段起始字段、正文尾段拼接和短参数默认值；不在文档记录正文 |
| `stage` / `transition` | 6538 / 6556 | 已确认前景、背景、坐标和最多十组 stand pair；stand position 与 transition 动画时序仍未闭合 |
| `label` / `goto` / `if` | 34 / 10 / 20 | 所有本地 target 均闭合，无重复 label |
| `chain` | 55 | 55 个 operand 均匹配同包脚本名；原程序处理函数把目标写入全局 `NEXT` 后结束当前脚本，是尾链式切换，不建立返回栈 |
| `select` | 2 | token 已知，choice/target operand 语义未知 |
| `end` | 85 | 75 个位于文件末尾；原程序与 `chain` 共用结束流程，空 `NEXT` 时终止，不是 return |

Minori 脚本研究以 `scr.paz` 中解出的 `.sc` 为核心。`perseus_chs.mys` 是本地样本的中文 patch/映射证据，不能当作原生 runtime source。

## 预期脚本单元

| 单元 | 用途 |
| --- | --- |
| `.sc` | 编译脚本，包含 command、jump、message、select 和资源引用 |
| `.mys` | 本地化 patch 数据或替换索引 |
| `.acr` | 历史工具中出现的翻译包形态 |

## 原程序反编译结论

本地原版入口的 SHA-256 为 `f5d0dd7feaff814093325e48059b3b6398dd554bdedbab16ea48b086ea77523a`。IDA 的 RTTI、vtable 和调用点共同确认了 command registry、tokenizer 以及下列 command handler；以下结论只适用于该 binary identity。

- `CommandChain` 只接受一个参数。执行时先调用与 `CommandEnd` 相同的结束流程，再保存 global store，并以该参数的值设置全局 `NEXT`。这里没有压栈、返回地址或恢复 caller 的路径。
- `CommandEnd` 只调用结束流程。此前讨论的“call stack 非空则 return”没有二进制依据，已撤销。
- `CommandSet` 与 `CommandSetGlobal` 使用同一 assignment evaluator，但分别绑定两个 store。变量解析先查 local store，再查 global store。
- assignment handler 接受 3 个或 5 个 token。3-token 形式直接赋值；5-token 形式还会计算 `|`、`&`、`+`、`-`、`*`、`/`、`%`。变量读取顺序已经确认；字符串赋值和除零行为还需结合真实 operand census 验证，当前 VM 不扩大支持范围。
- `CommandWait` 把整数写入引擎 timer slot；multimedia timer 以 10 ms 为基本周期递减该值。因此 `.wait 20` 表示 20 个 timer tick，也就是 200 ms，不是 20 ms。
- `CommandMessage` 在 positional operand 不少于四个时，把第一个 operand 解析为 message id，第二个保存为 voice identity，第三个保存为 speaker，第四个起以单个 ASCII space 重新连接为正文。连续分隔符形成的空 voice/speaker 必须保留。operand 不足四个时，parse handler 不写字段，但 execute 仍以构造器默认值 `id=-1` 和三个空字符串提交一次空消息更新。
- `CommandTransition` 接收 integer、string、integer 三个 operand，并把它们保存为后续 stage 的 mode、可选资源与 duration tick。单字符 `*` 走原程序的默认 transition 分支；具名 transition 的像素时序仍未接入 host，因此不能声明转场动画完成。
- `CommandStage` 的顺序已经由 parser 和 stage core 调用共同确认：前景资源、可选前景 `x/y`、背景资源、背景 `x/y`，随后是最多十组 `stand resource[,offset] + position`。`*` 表示该层无资源。背景和前景绑定 `bg` role；stand 绑定 `st` role。stand position 是引擎参数，不是像素坐标，当前遇到 stand 时继续阻断。
- `CommandPlayBGM` 与三个 SE command 已确认各自 parse/execute handler。资源 operand 不能原样拼成 URI，必须先经过下一条所述的 metadata 解析，再做精确 VFS 绑定。
- 音频资源规格现已确认是 `resource[volume,pan]`。volume 缺失时为 100，并夹在 0–100；pan 缺失时为 0，并夹在 -100–100。BGM 的两个整数依次控制新流 fade-in/音量过渡和旧流 fade-out；SE 的 boolean 由首字节是否为 `t` 决定，随后两个整数也按 fade-in、fade-out 传入各自 bus。
- census v3 对 811 条 BGM/SE/voice 引用做了 VFS 绑定测量：401 条非控制引用全部精确、唯一命中对应 archive entry；其余 410 条全部是 `*`。IDA 已确认 BGM、三个 SE bus 与 `playvoice` 的 `*` 都先停止各自固定 stream，并使用对应 fade-out 参数；runtime 不再把它当资源名。

## 反编译目标

输出中间 IR：

```text
ScriptFile
  labels[]
  blocks[]
  commands[]
  text_spans[]
  choice_groups[]
  resource_refs[]
```

`text_spans` 记录 message id、speaker、body、voice identity、source offset 和 raw length；商业正文仅存在于进程内的有界 lease 和 ignored 私有研究结果。当前 `commands` 还保留原始 command token、operand bytes、行 bytes 和已经验证的 CFG 分类。

## AstraVN 参考价值

Minori 的演出脚本要重点抽取：

- message 与 voice 的绑定方式。
- 背景/立绘变更和 transition 参数。
- wait/click/auto/skip 的状态条件。
- choice group 的变量写入和 route jump。
- system menu、backlog、save/load 对脚本 VM 的暂停点。

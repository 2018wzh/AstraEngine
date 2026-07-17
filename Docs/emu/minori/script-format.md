# Minori Script Format

## 当前 parser contract

当前样本的 `.sc` 不是二进制 framed bytecode，而是 CP932、CRLF 行式源码。89 个文件共 33728 行，其中 33695 行为 `.command operands`，另有 28 行注释和 5 行空白；旧的 `opcode:u16 + operand_size:u16` 假设已被样本否定并删除。

parser 逐字节保留原始行与换行，同时记录 command ordinal、source span、ASCII command token 和 raw operand。只有显式 `ScOpcodeCatalog` 中登记且经结构验证的 command 才获得控制流语义；未知 command 或未确认的 `select` 分支语义保留为 `Unknown`。

已实现的阻断项包括非法 CP932、重复 command spec、重复 label、无效本地 jump target、operand schema mismatch 和 lossless source invariant。`encode_sc(parse_sc(bytes))` 是 round-trip 门禁。完整正文与近源码 disassembly 只写 ignored 私有研究目录。

## 全包 census

| 分类 | 结果 | 证据边界 |
| --- | ---: | --- |
| `.sc` 文件 | 89 | 当前本地样本全包 |
| command | 33695 | 29 个已知 token，未知 token 为 0 |
| `message` | 18319 | 只确认 token 和 operand byte span，不在文档记录正文 |
| `stage` / `transition` | 6538 / 6556 | 参数语义仍需下一阶段分类 |
| `label` / `goto` / `if` | 34 / 10 / 20 | 所有本地 target 均闭合，无重复 label |
| `chain` | 55 | 55 个 operand 均匹配同包脚本名，记为外部 script call |
| `select` | 2 | token 已知，choice/target operand 语义未知 |
| `end` | 85 | 75 个位于文件末尾；当前记为 terminate，不冒充 return |

Minori 脚本研究以 `scr.paz` 中解出的 `.sc` 为核心。`perseus_chs.mys` 是本地样本的中文 patch/映射证据，不能当作原生 runtime source。

## 预期脚本单元

| 单元 | 用途 |
| --- | --- |
| `.sc` | 编译脚本，包含 command、jump、message、select 和资源引用 |
| `.mys` | 本地化 patch 数据或替换索引 |
| `.acr` | 历史工具中出现的翻译包形态 |

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

`text_spans` 未来记录 speaker、body、voice id、source offset 和 raw length。当前 `commands` 只保留原始 command token、operand bytes、行 bytes 和已经验证的 CFG 分类。

## AstraVN 参考价值

Minori 的演出脚本要重点抽取：

- message 与 voice 的绑定方式。
- 背景/立绘变更和 transition 参数。
- wait/click/auto/skip 的状态条件。
- choice group 的变量写入和 route jump。
- system menu、backlog、save/load 对脚本 VM 的暂停点。

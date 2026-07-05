# Minori Script Format

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

`text_spans` 记录 speaker、body、voice id、source offset 和 raw length。`commands` 保留原始 opcode、operand bytes 和已识别 command name。

## AstraVN 参考价值

Minori 的演出脚本要重点抽取：

- message 与 voice 的绑定方式。
- 背景/立绘变更和 transition 参数。
- wait/click/auto/skip 的状态条件。
- choice group 的变量写入和 route jump。
- system menu、backlog、save/load 对脚本 VM 的暂停点。

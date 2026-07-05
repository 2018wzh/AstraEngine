# Script Format

KrKr family 同时使用 TJS、KAG `.ks` 和编译/二进制场景。AstraEMU 不能只实现一个文本 KAG parser。

## TJS

TJS 是 KrKr 的系统脚本语言。样本里有两类：

| 类型 | 判断 | 例子 |
| --- | --- | --- |
| source | UTF-16 text，常见 BOM `FF FE` | `patchAI.xp3` 中的 `default.tjs`、`yuzu_default.tjs`、`appconfig.tjs` |
| bytecode | `TJS2100` magic | 根目录 `patch.tjs`、部分 `override.tjs` |

source TJS 中观察到 `SystemConfig`、`kag`、`sf`、`tf`、`Plugins.link("toml.dll")`、`KAGLoadScript("yuzu_default.tjs")` 等结构。bytecode 只能作为 TJS VM 输入或静态 metadata，不能按 source 格式处理。

## KAG `.ks`

`.ks` 是 KAG scenario/source。样本 `data.xp3` 中有：

- `scenario/start.ks`
- `scenario/macro.ks`
- `scenario/replay.ks`
- `main/custom.ks`
- `main/sysmenu.ks`
- `main/version.ks`

KAG parser 至少要识别四类行：

| 行型 | 例子 | 语义 |
| --- | --- | --- |
| label | `*entry` | 可跳转位置 |
| command | `@wait time=500` | 单行 tag |
| bracket tag | `[jump storage="next.ks" target="*entry"]` | 行内/独立 tag |
| text | `本文` | 进入 message layer |

上面的例子是自造格式样例，不来自商业 payload。实际 parser 要保留 source map：storage、line、label、tag name、attribute span 和 text span。`Krkr_text_out*.py` 的经验说明，翻译/抽取工具通常用“非 `;/*/@/[` 开头的行是文本”这类启发式；AstraEMU 不能只靠这个启发式执行脚本，必须经过 KAG grammar 和 tag handler。

## `.ks.scn`

样本的主线场景集中在 `scn.xp3`，共有 138 个 `.ks.scn`。`patchAI.xp3` 中同名 `.ks.scn` 的 payload 头部是：

```text
50 53 42 00
```

也就是 `PSB\0`。这类文件不能按 `.ks` 文本解析。初期处理策略：

1. archive reader 只读取 metadata 和 hash。
2. PSB probe 识别 header、version、name tree、string table、resource table。
3. 如果没有执行器，就把该 storage 标成 `unsupported_binary_scenario`，但仍保留 layer 覆盖关系。
4. KrKr compat core 后续要么实现 PSB scenario executor，要么通过旧 VM trace 输出 KAG 等价事件。

## PSB 参考字段

`M2Psb/PSBReader*` 里记录的 `psbinfo_t` 说明 PSB 至少包含：

- `nNameTree`
- `nStrOffList`
- `nStrRes`
- `nDibOffList`
- `nDibSizeList`
- `nDibRes`
- `nResIndexTree`

对 `.ks.scn`，这些字段先用于识别和诊断。不要默认它和图片 PSB 完全同构；执行语义要由 KrKr core 验证。

## 关联文本和配置

样本还包含 `.sli`、`.stage`、`.pbd`、`.sinfo`、`.toml`、`.ini`、`.csv`、`.mchx`、`.tft` 等文件。它们通常是媒体 timing、stage、UI layout、字体、配置或索引。AstraEMU probe 应记录这些扩展名和来源 archive，但不把未知格式硬塞进 KAG/TJS parser。

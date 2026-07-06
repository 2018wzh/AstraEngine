# Tooling

本页是 KrKr family 需要的只读工具接口。当前仓库提供 `Tools/AstraEMU/krkr_*.py` 研究脚本；后续 Rust core 可以复用同样的输入输出字段。

## `krkr_probe.py`

扫描 case 根目录，输出本地结构化 JSON report：

```bash
python Tools/AstraEMU/krkr_probe.py <case-root> --json
```

Report 包含 archive、plugin、savedata、standalone TJS、layer、media capability 和 diagnostics。默认不提取 payload。

## `krkr_xp3.py`

读取 XP3 index，输出 storage metadata：

```bash
python Tools/AstraEMU/krkr_xp3.py data.xp3 --json
python Tools/AstraEMU/krkr_xp3.py data.xp3 --out <extract-dir>
```

字段包括 storage name、normalized key、size、flags、segment count、segment flag、Adler-32、archive source。`--list-only` 不解出 payload。

## Layer Diff

对多个 archive 生成覆盖报告是 core 阶段工作。Python 阶段先用 `krkr_probe.py` 的 pack 顺序和 entry list 手工比对。

## PSB Probe

识别 `.ks.scn`/PSB，不输出正文：

```bash
python Tools/AstraEMU/krkr_xp3.py scn.xp3 --json
```

输出 header、version、table offset、resource section、hash 和是否支持执行。未知字段保留十六进制 offset，不猜剧情语义。

## `krkr_ks.py`

对解出的 `.ks` 文本抽取 label、tag 和文本行：

```bash
python Tools/AstraEMU/krkr_ks.py scenario.ks --json
```

`.ks.scn` 字节码和 PSB 需要后续专门 decoder。当前工具不会输出截图或音频采样。

## 参考脚本处理

`FuckGalEngine/Krkr` 里的参考工具可以作为读代码材料：

- `XP3Viewer-121113`：XP3/TLG 结构。
- `Krkr_text_out*.py`、`Krkr_text_in.py`：KAG 文本抽取/回写经验。
- `M2Psb`：PSB 表结构观察。
- `Conductor.tjs`：KAG tag conductor 模型。

不要把 KRPatch、hook、decrypt 笔记做成 AstraEMU 工具。兼容工具只做合法输入的读取、诊断、trace 和 provider 能力映射。

## 自检

工具实现后至少提供一个无商业 payload 的 synthetic case：

```text
case/
  data.xp3
    scenario/start.ks
    default.tjs
    image/test.png
```

自检断言：

- XP3 index 可读。
- virtual storage 找到 `scenario/start.ks`。
- KAG parser 识别 label、command、text、jump。
- trace 输出 boot、load script、dispatch tag、wait。
- report 不含原始文本正文。

# Siglus Tooling

本页只列安全 probe 和开发验证工具。对商业安装目录运行时，只允许读取 header、计数、索引、资源名和 hash；不要导出脚本文本、图片、音频或视频。

## AstraEngine 本仓工具

| 工具 | 作用 |
| --- | --- |
| `Tools/AstraEMU/siglus_probe.py` | 枚举扩展名，检测 `Scene.pck` 和 `Gameexe.*` header |
| `Tools/AstraEMU/siglus_scene.py` | 读取 `Scene.pck` package header |
| `Tools/AstraEMU/siglus_gameexe.py` | 读取 `Gameexe.*` 前 8 bytes 和可见 ASCII 片段 |
| `Tools/AstraEMU/common.py` | little-endian reader、magic label、safe archive helper |

安全命令：

```bash
python Tools/AstraEMU/siglus_probe.py "<game-root>" --json
python Tools/AstraEMU/siglus_scene.py "<game-root>/Scene.pck" --json
python Tools/AstraEMU/siglus_gameexe.py "<game-root>/Gameexe.dat" --json
```

这些命令不写输出目录，不导出 payload。

## siglus_rs 参考工具

| crate/bin | 安全用法 |
| --- | --- |
| `siglus_ss_decompiler` | 可用于自制 fixture 或公开测试数据；商业样本只建议 `--list` |
| `siglus_gameexe_ini` | 参考 `Gameexe.dat` decode pipeline；商业样本不要把展开后的 config 提交进仓库 |
| `siglus_g00_extract` | 只在自制资源上做 decode 验证 |
| `siglus_scene_vm` | 参考 VM 行为和 runtime boundary |

安全命令例子：

```bash
cargo run -p siglus_ss_decompiler -- --scene-pck path/to/Scene.pck --list
```

不在项目文档中记录 key、key 获取方式或完整导出命令。

## 旧工具代码

`FuckGalEngine/SiglusEngine` 中的 `SceUnPacker`、`ScePacker`、`stringdump`、`stringpacker` 和 patcher 代码只能作为结构参考。不要复用其中的 patch 注入、DLL hook、静态 byte table 输出或复包流程。

可用事实：

| 文件 | 可用事实 |
| --- | --- |
| `SceUnPacker/Main.cpp` | `SCENEHEADER` 字段顺序、scene name/data index 口径 |
| `stringdump/Main.cpp` | 字符串表 index 和 per-index UTF-16 XOR |
| `stringpacker/Main.cpp` | 字符串表 offset/length 单位 |
| `g00_test.cpp` | G00 type 0/type 2 header 和 LZSS/chip 结构 |

## 推荐验证

文档改动后运行：

```bash
python Tools/check_docs.py
```

Siglus core 实现后再加最小 fixture：

```text
fixtures/siglus/minimal_scene_pck/
  Scene.pck
  Gameexe.dat
  g00/minimal.g00
  scenario.yaml
```

Fixture 必须使用自制内容或可公开测试数据。

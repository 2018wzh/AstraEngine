# Minori Tooling

## `minori_probe.py`

```bash
python Tools/AstraEMU/minori_probe.py "E:\Games\夏空的英仙座（夏空のペルセウス）" --json
```

输出 `.paz`、`.mys`、`.exe`、`.chm` 的大小和 magic 标签。

## `minori_paz.py`

```bash
python Tools/AstraEMU/minori_paz.py "E:\Games\夏空的英仙座（夏空のペルセウス）\scr.paz" --json
python Tools/AstraEMU/minori_paz.py scr.paz --key-file local-key.hex --json
```

该工具不内置 key。没有 `--key-file` 时只输出 probe 信息。

## `minori_sc.py`

```bash
python Tools/AstraEMU/minori_sc.py decoded.sc --json
```

输入必须是已经合法解出的 `.sc` 或同等文本/二进制脚本片段。输出 message/select/voice/bgm/se/image 等 marker。

## 约束

所有 extract/decode 产物只能写到显式输出目录，不提交到仓库。

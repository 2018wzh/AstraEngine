# Minori Game Observations

## `夏空のペルセウス`

本地路径：

```text
E:\Games\夏空的英仙座（夏空のペルセウス）
```

文件事实：

| 文件 | 大小 | 说明 |
| --- | ---: | --- |
| `perseus.exe` | 1875456 | 原始入口候选 |
| `夏空的英仙座.exe` | 1507595 | 本地化入口候选 |
| `scr.paz` | 1914452 | 脚本 archive |
| `st.paz` | 852999948 | 图像 archive |
| `sys.paz` | 32418204 | 系统资源 archive |
| `se.paz` | 13554180 | SE archive |
| `voice.paz` | 326048276 | voice archive |
| `mov.paz` | 0 | 空 movie archive |
| `perseus_chs.mys` | 2064280 | 本地化 patch 数据 |

## 研究命令

```bash
python Tools/AstraEMU/minori_probe.py "E:\Games\夏空的英仙座（夏空のペルセウス）" --json
python Tools/AstraEMU/minori_paz.py "E:\Games\夏空的英仙座（夏空のペルセウス）\scr.paz" --json
```

预期输出包含 PAZ 文件列表、大小、hash、head bytes 和 `key_supplied=false`。

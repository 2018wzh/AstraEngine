# Minori Game Observations

## 当前样本

- 根目录存在 `scr/st/sys/se/voice/mov.paz` 六个角色名。
- 六个 archive 均非空。使用本地私有补丁后，六个 index 均已解密并通过结构校验。
- 已验证的 entry 数依次为 `scr=89`、`st=2321`、`sys=302`、`se=73`、`voice=7047`、`mov=5`，合计 9837。

## 未知

六包 9837 个 entry 已完成首段和尾段随机读取；首轮 cache 为 3526 hits/6311 misses，第二轮同 identity 为 9837 hits/0 misses。`scr.paz` 的 89 文件全包 census 已确认 CP932 行式结构与 29 个 command token；`select` 等 operand 语义仍待下一阶段确认。

## `夏空のペルセウス`

本地路径：

```text
<minori-case-root>
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
| `mov.paz` | 882835526 | movie archive |
| `perseus_chs.mys` | 2064280 | 本地化 patch 数据 |

## 研究命令

```bash
python Tools/AstraEMU/minori_probe.py "<minori-case-root>" --json
python Tools/AstraEMU/minori_paz.py "<minori-case-root>/scr.paz" --json
```

预期输出包含 PAZ 文件列表、大小、hash、head bytes 和 `key_supplied=false`。

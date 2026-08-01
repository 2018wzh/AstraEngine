# Minori Game Observations

## 当前样本

- 根目录存在 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive。
- `bg` 由 `bg.paz` 与 `bg.pazA` 至 `bg.pazJ` 组成；全目录共 18 个 PAZ 物理文件、5742470010 bytes。
- 八个 archive 均非空。使用本地私有补丁后，八个 index 均已解密并通过结构校验。
- 已验证的 entry 数为 `bg=4616`、`bgm=49`、`scr=89`、`st=2321`、`sys=302`、`se=73`、`voice=7047`、`mov=5`，合计 14502。

## 未知

manifest v2 full verify 已完整流读八包 14502 个 entry，并复读每个非空 entry 的首尾最多 4 KiB：共 43818 次 range read、6624958365 个 decoded bytes，aggregate hash 为 `sha256:2ff4eef4c496a04643aa703a9bd0c22d27a626d77968b6e11ea954397cbdb640`。该轮使用显式 no-cache profile，`cache_hit_count=0`。启用 cache 的前一次尝试因平台缓存卷空间不足，在首个 cache write 处阻断；没有改用隐式目录。`scr.paz` 的 89 文件 census 记录 33728 行、33695 个 command、29 个 command token，unknown opcode 为 0；`select` 等 operand 语义仍待确认。

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
| `bg.paz` + A–J | 3347405076 | 背景资源 archive，共 11 个物理卷 |
| `bgm.paz` | 285294348 | BGM archive |
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

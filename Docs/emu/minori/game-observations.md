# Minori Game Observations

## 当前样本

2026-08-29 起，Minori 已硬切为 `key.toml` 与无明文缓存的流式解密。此前 private-profile/cache profile 下的八包 full verify 只保留为历史观察；当前 reader identity 已在 2026-08-30 重新完成八包 full verify。重复密文读取有合成回归，峰值内存规模验证仍待执行。

- 根目录存在 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive。
- `bg` 由 `bg.paz` 与 `bg.pazA` 至 `bg.pazJ` 组成；全目录共 18 个 PAZ 物理文件、5742470010 bytes。
- 八个 archive 均非空。当前 key-file identity 已通过八个 index 和全部 entry 的 full verify。
- 已验证的 entry 数为 `bg=4616`、`bgm=49`、`scr=89`、`st=2321`、`sys=302`、`se=73`、`voice=7047`、`mov=5`，合计 14502。

## 未知

当前 manifest v3/key-file reader identity 已完整流读八包 14502 个 entry，并复读每个非空 entry 的首尾最多 4 KiB：共 43818 个逻辑读取范围、6624958365 个 decoded bytes，aggregate hash 为 `sha256:e641854399512fea4182ebc7de845436d37d3eaef0b31d748b41c8bd23f9e64b`。同一 identity 的 `scr.paz` census 包含 89 个文件、33728 行、33695 个 command 和 29 个 command token，unknown opcode 为 0；`select` 等 operand 语义仍待确认。

## `夏空のペルセウス`

### 原生确认框

剧情菜单中的 Game→Exit 与 Game→Return title 都由原版 Host 弹出两按钮确认框；
正文分别为 `終了してもよろしいですか?` 与
`ゲームを中断してメニューに戻ります。よろしいですか?`，按钮顺序为
`是(Y)`、`否(N)`。标题页主菜单的 Exit 直接结束程序，不经过该确认框。以上只记录
交互结构和文案，不把截图或商业资源写入仓库；Family ABI 的 confirmation transaction
已采用同样的边界。

### 右键系统菜单

2026 年 8 月 30 日在授权原版程序中确认：右键打开原生系统菜单，不会直接进入 Save 页。标题阶段包含窗口显示选项、Help 和 Game；剧情阶段在这些项目之前增加消息框显示、Auto、Skip、Quick Save、Save、Load 和 Config。Help 子菜单包含帮助、关于和 minori 主页，Game 子菜单包含返回标题和退出。转场期间的右键没有打开菜单。

消息框隐藏/显示与 Auto 已在原版现场操作确认。新的空白进度复测还确认，选择 Skip 后，当前未读消息不会自动推进；重新打开菜单时，Auto 和 Skip 都没有勾选标记，只有原版窗口缩放选项保留勾选。当前 runtime 因此只让持久 Skip 快进已记录 read identity 的消息；受 pragma 门控的 Control 仍是独立的按住快进路径。其余命令的完整副作用、确认框和持久化细节仍需继续观察或做针对性 IDA 核验；当前实现不能仅凭菜单文字猜测这些行为。该观察只记录菜单结构与已操作结果，不属于 Headless E2 或 Release Sandbox 验收。

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

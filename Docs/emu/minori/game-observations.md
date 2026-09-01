# Minori Game Observations

## 当前样本

2026-08-29 起，Minori 已硬切为 `key.toml` 与无明文缓存的流式解密。此前 private-profile/cache profile 下的八包 full verify 只保留为历史观察；当前 reader identity 已在 2026-08-30 重新完成八包 full verify。重复密文读取有合成回归，峰值内存规模验证仍待执行。

- 根目录存在 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive。
- `bg` 由 `bg.paz` 与 `bg.pazA` 至 `bg.pazJ` 组成；全目录共 18 个 PAZ 物理文件、5742470010 bytes。
- 八个 archive 均非空。当前 key-file identity 已通过八个 index 和全部 entry 的 full verify。
- 已验证的 entry 数为 `bg=4616`、`bgm=49`、`scr=89`、`st=2321`、`sys=302`、`se=73`、`voice=7047`、`mov=5`，合计 14502。

## 2026-09-01 原版标题与 System 页视觉基线

Windows Sandbox 中直接启动原版入口后，标题页在 1280×720 窗口内显示 16:9 内容，
左右保留黑色 pillarbox；右侧菜单顺序为 `New Game`、`Load Data`、`System`、`Exit`。
`System` 是游戏内页面：模糊角色/向日葵背景上分为 Message Speed、Screen Mode、
Volume、Visual、Font、Sound、Voice、Play Mode、Other 九组，含滑杆、MUTE/TEST、
全屏/窗口、Visual 开关、Auto/Skip、角色语音开关以及 `OK`/`Cancel`。这是原版页面
结构观察，不把它误记为 Host-native popup。

窗口标题在 Sandbox 默认非日文 code page 下出现乱码，但画面资源仍保持日文，说明
运行时应在 Host/字节边界绑定 `astra.emu.minori.locale.ja-jp.cp932.v1`；当前阶段不
加载汉化 exe、`.mys` 或翻译 overlay。对应私有截图的公开 hash 为标题
`sha256:eb3d18cbb0079f049729e222e6669ca0c5cdaad19973d44fcb135409b74c4bcf`、System
`sha256:780ee54a1be14b877d75fd80448e014cd320065f9ceaa6b8907bef01d3c9b300`；截图文件
和本地路径不进入仓库。

## 2026-09-01 原版首段剧情与右键菜单

从标题页选择 `New Game` 后，原版先显示黑场和中央竖排标题，再进入深蓝色底部消息
面板。首条日文消息带有下三角推进指示；Enter 只推进一条消息，未观察到自动跳过或
场景层级变化。该结构与当前 CP932 message publisher 的输入边界一致，但不作为完整
语音或 route 语义证据。

剧情等待期间右键会打开以物理指针为锚点的原生白色菜单。菜单包含消息框、Auto、Skip、
Quick Save、Save、Load、Config，以及 Help、Game 两个子菜单；Help 包含帮助、关于和
minori 主页，Game 包含返回标题和退出。关闭菜单后等待状态保持不变，本次没有触发保存、
跳转或退出。该观察支持 Host 处理 Family menu transaction，而不是由游戏 underlay
绘制弹窗或菜单。

Sandbox 非日文 code page 下窗口标题和原生菜单标签出现乱码，但游戏消息仍为可读日文；
因此当前移植继续只支持原版日文，并在 Host/字节边界使用
`astra.emu.minori.locale.ja-jp.cp932.v1` 转区 hook，不加载汉化 exe、`.mys` 或翻译
overlay。私有截图的公开 hash 为首条消息
`sha256:d36a32752924bcd20f7e96522f4a07013d2a098097043795c045f34cd2f99445`，右键菜单
`sha256:9cfd779a7cb5e042f8296cf4d5ac3bd1df9cdf3eaf483a6f299151af555ee7e4`；图片本身和
本地路径不进入仓库。

上述仅是原版现场行为观察，不关闭 Save 注释列表布局、Auto/Skip 完整副作用、四路线
自然结局、原版同点视觉比较或正式 E2/E3。

## 2026-09-01 System 页默认值

新标题会话的 System 页显示：未读、既读和 Auto 播放等待均为中位；BGM、语音、效果音
均为最大值且 MUTE 未选；窗口模式、画面效果、文字阴影和动画均开启。Sound 中 backlog
语音自动播放开启、进入下一句停止语音关闭；Play Mode 的 Auto 开启、Skip 关闭；Other
中的后台继续播放关闭，五个角色语音开关均开启。当前 `MinoriConfigState::default()`
与这些观察一致，并有 runtime 回归固定该行为。

对应私有截图的公开 hash 为
`sha256:50dc8af81ed1969897ea92aa2b5c76135cf08730c1fed278155a037cee28e8fd`；不提交图片、
本地路径或字体列表内容。该记录不替代持久化修改、平台音量绑定和原版同点视觉验收。

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

2026 年 8 月 31 日补充了标题页的原生菜单观察：Help→About 会在游戏窗口上方打开 owner-modal 对话框，显示作品图、版本和版权信息，并以单个 `OK` 按钮结束；关闭对话框后标题页与焦点保持不变。标题页切换到全屏后重新打开右键菜单时，全屏切换项消失，只保留原始尺寸、禁用的高精度尺寸调整、抗锯齿、Help 和 Game；选择原始尺寸退出全屏后，全屏项再次出现。该行为只作为 Host system-command 与 Family menu transaction 的观察依据，不把截图或商业资源写入仓库。

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

### 运行范围

表中的 `夏空的英仙座.exe` 与 `perseus_chs.mys` 说明样本目录同时保留过本地化材料，
不表示它们属于当前移植目标。runtime 只接受日文原版内容变体
`natsuzora-no-perseus.original-ja`，并要求非符号链接的 `perseus.exe`；本地化 exe、MYS
覆盖和汉化文本不会被探测、挂载或合并。`astra.emu.minori.locale.ja-jp.cp932.v1`
是严格的原版 CP932 转区绑定，只在字节边界执行日文解码/编码，不替换正文，也不提供
GBK/翻译回退。缺少该绑定、出现非法 CP932 或选择本地化变体时，mount 以稳定 diagnostic
阻断。

## 研究命令

```bash
python Tools/AstraEMU/minori_probe.py "<minori-case-root>" --json
python Tools/AstraEMU/minori_paz.py "<minori-case-root>/scr.paz" --json
```

预期输出包含 PAZ 文件列表、大小、hash、head bytes 和 `key_supplied=false`。

## 2026-09-01 Save/Load 占用槽观察

原版 Save/Load 页每页显示十个槽。空槽在白色卡片左侧显示灰色 `nodata` 图块；
占用槽保留同一位置的 96x54 预览图，并在右侧以红色显示本地时间
`YYYY/MM/DD HH:MM` 和用户注释。Save 页首屏只显示 `Next`、`Return`，Load 页还显示
`Back`；页标签由 `saveload_Page0..9.png` 提供，不能用 Quick/Auto 文本猜测替代。

当前 runtime 的 save envelope 已硬切为 `astra.emu.minori.save_slot.v3`，严格绑定
case/package/profile identity，并验证时间、注释、缩略图 PNG 尺寸和有界字节数。缩略图
从当前 1280x720 premultiplied gameplay surface 生成，metadata 经现有日文文本
presentation 通道发送；缺少本地时间或 gameplay surface 时保存阻断。此记录只描述
原版行为和脱敏实现边界，不处理汉化版，也不把截图或商业 payload 写入仓库。

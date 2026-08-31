# Minori 移植日志

## 2026 年 9 月 1 日：CP932 多字节尾字节与消息控制边界

原版脚本的消息正文先经过绑定的 Japanese CP932 locale hook，再交给消息控制
解析器。补充回归使用一个 CP932 编码后恰好以 `0x5c` 结尾的多字节字符，随后接
已确认的 `\\v\\a` 控制序列；解析结果保留字符本身，只把两个控制识别为等待语音与
自动推进。这样避免把 CP932 的多字节尾字节误当成 UTF-8 反斜杠或控制起始符。
该测试只使用合成文本，未把原始脚本内容、路径或 locale 数据写入报告。中文版本、
翻译 overlay 与其他编码仍不在本轮范围内。

## 2026 年 9 月 1 日：原版标题与 System 页基线（Windows Sandbox）

在授权 Windows Sandbox 中以日文原版入口启动同一份样本，记录了两个新的视觉基线。
标题页在 1280×720 窗口内保持 16:9 内容区，两侧有黑色 pillarbox；右侧主菜单按
`New Game`、`Load Data`、`System`、`Exit` 排列。进入 `System` 后仍是游戏自己的
全屏页面，而不是 Host 对话框：背景为模糊的角色与向日葵场景，页面分成 Message
Speed、Screen Mode、Volume、Visual、Font、Sound、Voice、Play Mode、Other 九组，
包含三个音量滑杆、MUTE/TEST、全屏/窗口、三项 Visual 开关、Auto/Skip 和五个角色
语音开关，底部有 `OK`/`Cancel`。

Sandbox 默认非日文 code page 下，窗口标题的 CP932 字节会显示为乱码，而游戏画面内
的日文资源仍可呈现；这进一步支持在 Host/字节边界绑定严格 CP932 locale hook，不能
用 GBK、替换字符或翻译 overlay 修补。该观察只记录布局和交互入口，截图留在 ignored
私有目录；公开证据仅保留尺寸与 hash（标题 `sha256:eb3d18cbb0079f049729e222e6669ca0c5cdaad19973d44fcb135409b74c4bcf`，
System `sha256:780ee54a1be14b877d75fd80448e014cd320065f9ceaa6b8907bef01d3c9b300`）。
尚未据此推断 Save 注释、逐字速度公式或四路线终局语义。

## 2026 年 8 月 31 日：标题系统菜单的 About 与全屏状态复核

在授权 Windows Sandbox 中重新打开原版标题页右键菜单，确认 Help→About 是附着
在游戏窗口上的 owner-modal 对话框，关闭后不会改变标题场景或焦点。该对话框显示
作品图、版本和版权信息，只有一个 `OK` 结果。随后在同一会话切换全屏并再次打开
菜单，原版隐藏全屏切换项；选择“ウインドウをオリジナルサイズに”退出全屏后，
下一次菜单重新出现该项。

Minori Family 继续只发布 `ShowAbout` 与 `RestoreOriginalSize` typed
`LegacySystemCommandTransactionV1`，由 Windows Host 负责 owner、modal focus、
窗口几何与原生对话框。该轮没有修改 VM 语义，也没有把截图、图片 payload 或本地
路径写入文档；它是 Release Sandbox 行为观察，不能替代 Headless E2、正式视觉审查
或 Windows E3。

## 2026 年 8 月 31 日：标题页退出回归

在已确认的原版行为基础上，补充 `title_exit_terminates_directly_without_confirmation_transaction`
回归。标题页的 Exit 现在由 Minori provider 直接结束 session，不发布 Family ABI confirmation；
剧情页的 `game_exit` 和 `game_return_title` 仍通过 Host 原生确认框处理。该项是 provider
行为回归，不能替代 Release Sandbox 视觉验收或 Windows E3。

## 2026 年 8 月 31 日：确认框标点与原版按钮事务对齐

在授权 Windows Sandbox 中复核剧情菜单的 Game→Exit 与 Game→Return title 后，确认框
正文使用 ASCII `?`，而不是全角问号；标题、`是(Y)`/`否(N)`按钮顺序和 owner-modal
行为保持不变。Minori Family ABI 现在按该精确文案发布，Host 继续只负责原生呈现、
焦点和结果回传。定向 provider 回归覆盖两种确认路径，未改变标题页直接 Exit 的行为。
这项修正只针对字符串与原版观察对齐，不提升完整路线、Release Sandbox 或 Windows E3
证据等级。

## 2026-08-31：原版菜单禁用项的键盘焦点语义

授权 Windows Sandbox 的标题右键菜单复核确认：方向键会把焦点移动到灰色的禁用项，
也会跳过分隔线；禁用项保持灰显，鼠标点击不会发布命令。这个行为与“只在
enabled 集合中循环”的旧 Headless/Manager 实现不同。现在 CLI 的物理菜单和 Manager
Host 都把非分隔线项按 ABI `order` 纳入焦点序列；Right/Space 对禁用项保持无操作，
Enter 关闭弹出菜单但不向 Family 发布该项选择；可用 submenu 和 command 仍按原有
事务路径处理。进入 Help submenu 后按 Left/Escape 会回到根菜单，并把焦点恢复到
Help 行，而不是跳到第一项。

该修复只调整 Host 导航状态机，不改变 Family transaction、命令校验或平台能力边界。
本次现场观察和 CLI/Manager 定向回归属于菜单 E1/E2 证据，真实 Manager 窗口、完整路线、
Release Sandbox 视觉验收和 Windows E3 仍未关闭。

## 2026-08-31：原始尺寸命令同步全屏状态

原版在全屏状态下选择“ウインドウをオリジナルサイズに”会先退出全屏，再恢复
窗口尺寸；之后重新打开窗口菜单时，全屏项重新出现。此前 Host 已完成窗口操作，
但 Family VM 仍保留旧的 `fullscreen` 配置，导致下一次菜单继续隐藏该项。现在只有
收到 Host 的 `Applied` 结果后，Minori 才清除 Family-owned 全屏状态并原子持久化
配置；`Rejected`/`Unsupported` 仍保持 blocking，不会提前修改 VM。新增回归覆盖
“切换全屏 → 原始尺寸 → 菜单重开”的 transaction 顺序。

该修复只同步 Host 的确定结果与 Family 菜单状态，不把窗口句柄、路径或商业资源
写入 ABI、存档、报告或日志。

## 2026-08-31：Windows 原生确认框尺寸与系统图标对齐

补充复核原版退出/返回标题确认框后，Windows Host 的 Family ABI presenter 采用
原版接近的 350×164（96 DPI）紧凑基准尺寸，并按 owner window 的有效 DPI 等比缩放
所有控件；无 owner 的 Manager service 调用使用系统 DPI。客户区的 question icon 和
消息均从约 `(26,28)`/`(64,28)` 开始，按钮位于约 `y=100` 的下方 command band。
消息左侧使用 Win32 系统 question icon，按钮仍保留 `是(Y)`/`否(N)`
助记键、owner-modal 禁用/恢复和关闭即取消语义。缩放计算使用 checked arithmetic，
超大 DPI 不会溢出；新增的平台单元回归覆盖 96/144/192 DPI 和饱和边界。

这项改动只调整 Host 原生呈现，不把图标、商业标题或截图写入 Family ABI、profile、
report 或日志。原版 About artwork、Linux 原生菜单和正式 Release Sandbox/E3 视觉
验收仍是独立开放项。

## 2026-08-31：Manager 菜单子层级导航

Family transaction 的 submenu 不能被当成命令点击。Manager 的 Host UI 现在保留每个
菜单项的 parent id，只显示当前 parent 的同级项；启用的 submenu 通过有界的 Host UI
导航进入，返回按钮回到已发布的父项，真正的 command 才会送回 Family ABI。适配器在
切换前再次核对 active menu、parent、kind 和 enabled 状态，并重跑菜单层级校验，循环、
断链或超过四层的树在进入 Slint 前直接返回稳定 diagnostic。这样 Manager 与 Windows/
macOS 原生 Host 的语义一致，也不会用一个假的 item id 绕过 Minori 的结果校验。

Manager 的菜单焦点也由 Host UI 保持：Up/Down 只在当前父节点的启用项中循环，
Right/Enter/Space 进入 submenu 或选择 command，Left/Escape 在子菜单返回、在根菜单
关闭。焦点键不会泄漏到 gameplay。该改动只修复宿主菜单导航和错误边界，不改变 Family
transaction 或原版菜单顺序；Linux 原生 context menu、同点截图、Release Sandbox、
完整路线和 Windows E3 仍保持开放。

## 2026-08-31：原版 native 菜单与 Release 包启动复核

授权原版在 Windows Sandbox 中进入标题后，右键会在指针位置打开包含窗口项、
Help 和 Game 子菜单的原生层级菜单；选择退出会显示由窗口拥有的 native 确认框，
取消后回到同一标题状态。该观察与 Family ABI v13 的菜单/确认 transaction 及
Host 原生处理路径一致，只记录交互结构，不把截图或商业资源写入仓库。

同一 session 开始新游戏后，菜单会增加消息控制、Quick Save、Save、Load、Config
以及 Auto/Skip/Control 分组；Game 子菜单的 Return-title 项同样通过 native
confirmation。取消会保留当前消息等待，接受后回到标题页，标题画面和菜单状态
重新出现。该行为已在 Sandbox 现场复核，仍不等价于完整路线或正式 E3 证据。

随后切换到全屏再打开右键菜单，原版会隐藏全屏切换项，只显示“原始尺寸”、禁用且
保持勾选的“高精度尺寸変更”和已勾选的抗锯齿项；选择“原始尺寸”回到窗口模式后，
全屏项重新出现。Minori provider 现在按实际窗口状态构造这组三项/四项菜单，并保留
完整活动 transaction 来校验 Host 返回的 item id、可选状态和命令类型；未发布、禁用
或 submenu/separator 项不会进入 VM。该结论来自现场菜单结构观察和定向 provider 回归，
不替代各平台原生窗口的正式验收。

首个 Release 候选在 Sandbox 启动时暴露了机器 VC runtime 依赖，随后 desktop
builder 强制 Windows MSVC 使用 `+crt-static`，并拒绝外部动态 CRT flag。新的
开发签名 Release 候选已能在同一 Sandbox 打开 AstraEMU Manager，PE import
检查未发现 `MSVCP140`/`VCRUNTIME140` 依赖。该结果只关闭分发包启动阻断；真实
Minori 路线、完整原版行为对齐、Headless E2 和 Windows E3 仍未完成。

## 2026-08-31：Windows Manager service host 原生确认框（初始实现）

Manager 的 Windows 音频/解码 lane 使用不创建 Winit 窗口的 service host；此前该 host
把 Family ABI confirmation 当成普通 window command 拒绝，导致 `game_exit` 和
`game_return_title` 在 Manager 中无法结束 pending transaction。现在 service thread
直接调用共享的 Windows native confirmation helper（无 parent），结果仍按
`Accepted`/`Cancelled` typed mapping 回送同一 Family session；菜单、全屏、帮助和
About 不会借此路径偷偷转交。

该项只关闭 Manager service-host 的确认事务悬挂问题；没有增加 fallback，也不替代
真实 Windows Release Sandbox/E3 视觉和键盘证据。

## 2026-08-31：Windows confirmation loader 边界修正

开发和 Release CLI 在加载 `rfd` 的 `common-controls-v6` Task Dialog 路径时会在进入
`main` 前退出（Windows `STATUS_ENTRYPOINT_NOT_FOUND`），因此该静态依赖不能作为
发布入口的一部分。现在根依赖移除该 feature，Windows Host 改用同一线程上的纯
Win32 `user32` modal window：保留 live parent caption、消息文本和 Minori 的
`是(Y)`/`否(N)`标签，关闭、键盘导航、owner 禁用/恢复与错误结果都在 Host 边界完成。
Manager 无窗口 service host 继续复用这条路径；`rfd` 仅保留 About 的普通 OK 对话框。

定向平台测试和 `astra-emu-cli --help` 启动检查通过。这只是加载器和确认框接线的
E1/E2 证据，真实对话框视觉、Release Sandbox 路线和 Windows E3 仍未完成。

## 2026-08-31：Windows 原生确认框快捷键边界

自定义 Win32 modal 现在在对话框消息循环入口消费 `VK_RETURN`/`VK_ESCAPE`，并把
原版按钮标题中的 `Y`/`N` 助记键分别映射为 `Accepted`/`Cancelled`。映射发生在
Host 的当前 confirmation transaction 内，不会把 `Y`/`N` 注入 Minori gameplay
输入；按钮控件仍保留 Tab/Enter 导航和关闭即取消的标准行为。新增的 key mapping
回归通过，真实 Sandbox 键盘证据和完整路线门禁仍未形成。

## 2026-08-31：Config 全屏设置改由 Host 原生应用

Config 页面提交全屏选项后，Minori 不再把窗口状态当作自己的即时副作用。family
先完成 draft apply，再通过 Family ABI v13 发布有界的
`LegacySystemCommandTransactionV1::SetFullscreen`；Host 在自己的平台事件循环中执行
窗口切换，并在后续 fixed tick 回送 typed result。命令等待期间禁止 gameplay 输入和其他
completion，避免同一 session 同时推进 VM 与 native window。

为避免 Config 关闭后留下旧的页面，family 会在命令发布轮重建一次 retained gameplay
presentation；Host 返回 `Applied` 后再更新运行时的 fullscreen 状态并写入
installation-scoped config。取消或没有实际改变 fullscreen 时不会发布命令；Rejected、
Unsupported、持久化失败仍直接返回 blocking diagnostic。新增 provider 回归覆盖 draft、
命令、挂起、完成和恢复链路，既有右键菜单窗口命令回归保持通过。

这仍是 provider/ABI 的局部 E1/E2 证据，不代表各桌面平台的原版窗口行为、同点视觉
对照或 Windows E3 已完成。

## 2026-08-31：原版消息推进指示器对齐

在授权 Windows Sandbox 的普通剧情画面中，正文末尾稳定出现一个白色下三角；
打开右键菜单或确认框时，底层消息层仍保留该标记。它不是脚本文字，也不是
`\\v`/`\\a` 的可见字符。该观察只记录画面结构，截图和原始脚本继续留在 ignored
私有研究目录。

Minori text surface 现通过现有 CosmicText/Renderer2D 路径追加独立的 `U+25BC`
glyph run。正文仍单独进入翻译 Hook、backlog 和一次性 text lease，指示器只保留
在本次 Host-owned raster surface；Noto Sans JP 的显式 coverage 同步声明该 glyph。
这修复了此前把控制序列末尾错误呈现为普通字符的路径，并新增有/无指示器的 raster
回归。出现时机、精确位置和动画节奏还需在干净原版与同一 Headless checkpoint
逐点对齐，因此当前不宣称像素 parity。

## 2026-08-31：Family ABI 原生菜单 Host 分层

继续、退出确认框和剧情右键菜单继续只通过 Family ABI v13 的 typed transaction
跨边界：Minori 负责语义、层级、勾选状态和结果校验，Host 负责呈现与物理输入，
不在 Manager 或 CLI 复制一套菜单逻辑。Windows 继续使用 `muda-win`/`rfd` 的
原生控件；macOS Host 新增 AppKit `muda` context menu，按 winit flipped view
坐标转换显式锚点，并在主线程完成菜单追踪；Headless 保留物理方向键和确认键路径。
当前 Linux Wayland Host 没有 GTK window 绑定，原生 context menu 仍明确返回
`ASTRA_EMU_PLATFORM_CONTEXT_MENU_UNSUPPORTED`，不会留下悬挂 transaction 或静默切换
到另一套 UI。该项仅证明 Host 分层和平台边界，完整路线、媒体、正式视觉对照和
Windows E3 仍保持原有证据状态。

## 2026-08-31：原生菜单选择回执的 Host 校验

Windows 和 macOS Host 在 `muda` 返回选择后，会把事件 id 与当前 Family ABI
transaction 的 item 集合重新比对，只接受启用的 `Command`；过期事件、未知 id、
submenu、separator 或已禁用项都在 Host 边界返回稳定错误，不会送回 Minori session。
这层校验与 Family 自身的重复/错配检查互补，避免进程级 `MenuEvent` 队列中的迟到事件
污染下一次右键菜单。该改动只增加错误可见性和平台安全边界，没有改变原版菜单顺序或
标签，也没有把 Linux 未绑定 GTK window 的 context-menu 能力伪装成已支持。

## 2026-08-31：原生菜单分类标题对齐

在授权 Windows Sandbox 中重新打开剧情右键菜单，确认底部两个顶层分类带有日文标题和
键盘助记符，而不是英文标题。Minori family menu transaction 现发布
`ヘルプ (&H)` 与 `ゲーム (&G)`，其余顺序、分隔线、禁用项和勾选项保持原观察结果。
Windows Host 继续只负责构建和呈现菜单，不按标签解释命令；该观察只写入脱敏的标签和
结构结论，截图仍留在 ignored 私有研究目录。

## 2026-08-31：Windows 原生确认框标题对齐

在授权 Windows Sandbox 中重新观察窗口关闭确认框：原版确认框的标题栏沿用游戏
窗口标题，而不是使用消息正文的短标题。Windows platform provider 现在在有 parent
window 时读取该窗口的 live caption，并把它交给 native message dialog；没有 parent
的 service-only 调用仍使用 Family ABI 传入的 title。该调整不把商业标题写入 Family
ABI、CLI、report 或日志，截图继续留在 ignored 私有目录。

## 2026-08-31：确认框键盘语义

确认框的键盘语义已补齐：原版按钮显示 `是(Y)` 与 `否(N)`，Headless Host 仅在
Family ABI confirmation transaction 激活期间接受 `Y`/`N`，分别回送 typed
`Accepted`/`Cancelled`。事务结束后 Y/N 仍是未绑定键并被拒绝；这保证系统快捷键不会
绕过剧情输入。Windows 原生对话框仍由平台 provider 负责键盘处理。该改动有 CLI
映射回归，但不构成完整路线、Release Sandbox 或 Windows E3 证据。

## 2026-08-31：新输入契约下的完整路线 Release Headless smoke

- 使用当前开发签名 Release CLI、显式 WMF provider 和重新生成的物理输入序列，
  从标题连续消费 195 条输入消息。输入覆盖 48 个消息边界、48 个 checkpoint，
  并在路线尾部显式结束会话；报告完成 24048 fixed steps、109 个提交/栅格帧，
  diagnostic 为空，全部输入序列已消费。
- 该序列包含一次短暂的 Control 快进按键，用于复核影片阶段不会把同一输入重复解释为
  普通消息确认。此次使用稀疏 frame sampling，且尾部是显式 shutdown，不把会话状态
  误报为 terminal；没有把它当作 120 Hz GPU E2、Release Sandbox 或 Windows E3。
- 这次 smoke 只证明现行 `runtime.input_or_terminal` 观察键、完整输入排序和当前
  WMF/Family ABI v13 接线可以跑完既定路线脚本。四路线自然解锁、原版同点视觉、
  音频人工听审、save/restore required checkpoint 和正式平台验收仍保持开放。

## 2026-08-31：原版确认框文案与输入观察边界

- 在干净的原版 Windows Sandbox 会话中分别打开 Game→Exit 和
  Game→Return title。两者都使用原生两按钮确认框，按钮顺序为
  `是(Y)`、`否(N)`；退出正文为 `終了してもよろしいですか?`，返回标题正文为
  `ゲームを中断してメニューに戻ります。よろしいですか?`。取消后舞台和当前消息
  保持不变，接受才结束当前 session 或回到标题。截图只留在 ignored 私有研究目录，
  本页不保存图片或路径。
- Minori provider 的 Family ABI v13 confirmation transaction 已采用上述原版
  日文文案和按钮顺序；Host 继续负责 native dialog 的 parent、焦点和结果回传，
  family 只处理 `Accepted`/`Cancelled` 语义。该项覆盖退出、返回标题和
  `window.close` 三个入口，未把确认框改成 Manager 自绘控件。
- 当前 Headless 输入契约用 `runtime.input_or_terminal` 表示可消费的输入边界；
  旧私有序列中的 `runtime.awaiting_input` 不是现行观察键，已按 hard-cut 规则
  拒绝，不增加兼容别名。用新观察键重排后的路线输入仍需完成完整 Release
  Sandbox 与正式 Windows E3 验收，当前不扩大证据等级。

## 2026-08-30：Family ABI v12 确认事务与平台 Host

- `game_exit`、`game_return_title` 和 Host 的 `window.close` 现在都由 Minori 通过 Family ABI 发布有界 `LegacyConfirmationTransactionV1`。Host 保存一次性 pending transaction，按 session 严格匹配结果；取消只恢复原有 wait，接受才进入 terminal 或返回标题。确认期间的 gameplay input、system-menu result、重复结果和错配 id 直接阻断。
- `astra-platform` 新增 `PlatformHostClient::show_confirmation` 与 `HostCommand::ShowConfirmation`。Windows、Linux 和 macOS 走各自的 `rfd` native message dialog，使用可选 parent window 与有界标题/正文/按钮；Headless、Android、Web 明确返回 `PLATFORM_NOT_IMPLEMENTED`，由 Headless CLI 的物理方向键、Enter/Space/Escape 路径完成确定性验证。Manager 只组合 Host service，不再绘制第二套 confirmation 语义。
- Family API v12 wire、Manager Core confirmation host、动态 loader、CLI close event、Minori provider 和平台 Host 的定向测试通过。该结果是当前 ABI 的 E1/E2 接线证据；尚未形成 Release Sandbox 行为验收、120 Hz 性能证据或 Windows E3。

## 2026-08-30：原生菜单 Save、Load 与 Config 的 Release E2

- 开发签名 Release v24 从空白进度启动，用 secondary-pointer、方向键、Enter 和 Escape 依次打开 Save、Load 与 gameplay Config。报告通过 138 fixed steps、82 条物理输入、9 个呈现帧和 8 个 checkpoint，diagnostic 为空。三个页面关闭后都返回同一条剧情消息，前后四张 gameplay PNG 字节一致。
- 复验暴露了两处页面所有权错误。Load 页曾按 session 的 `Title` launch mode 判断返回目标，但剧情也是从标题启动，Escape 因而误回标题；现改由保留的 gameplay wait 区分标题 Load 与剧情 Load。Config Cancel 已先把页面改回 gameplay，provider 却仍按 system-page tick 推进，随后触发非法状态；现改由 provider tick 接管关闭后的提交，并恢复底层消息 wait。
- 模型查看了全部 checkpoint。标题、Save、Load、Config 和恢复后的剧情画面都使用真实系统资源；日文字形、比例、层次、透明度和焦点没有发现裁剪、拉伸或残影。Save/Load 当前空槽、分页和返回行为得到定向覆盖，实际写入槽、跨 session Load 仍沿用既有独立 E2，本次不重复声明。
- 该结果关闭当前身份下右键菜单进入 Save/Load/Config 并返回剧情的定向 Headless E2。原版窗口关闭确认、`game_return_title` 确认语义、Release Sandbox、120 Hz 性能门禁和 Windows E3 仍未完成。

## 2026-08-30：原生菜单 Auto 的 Release E2

- Release CLI 的 Headless host 原先没有实现 Manager 已有的消息 wait 重绑规则：Auto 将活动消息从 `Input` 改为 `Time` 时，CLI 把同一 token 误判为重复 wait。规则现已收敛到 Manager Core，RuntimeWorld adapter、Manager 和 CLI 共同只允许 `Input` 与 `Time` 互换以及 `Time` deadline 更新；同批重复、`Input` 重发和其他 wait 类型仍阻断。
- 开发签名 Release v21 从空白进度启动，通过 secondary-pointer 和方向键在 family v11 菜单中选择 Auto。报告通过 371 fixed steps、36 条物理输入、9 个呈现帧、6 个 checkpoint，diagnostic 为空。Auto 开启后 2 秒内从第一条消息推进到下一场景；再次选择 Auto 恢复 Normal 后，继续运行 2 秒的画面与关闭时完全一致。
- 三个关键画面已人工检查，未见缺字、裁剪、拉伸或明显图层错误。该结果关闭当前身份的原生菜单 Auto 定向 Headless E2，不替代完整路线、Release Sandbox、120 Hz 性能门禁或 Windows E3。
- 原版 Sandbox 的窗口关闭按钮会弹出原生确认框，正文与舞台保持在其后方；取消后 session 继续。该观察与当前 Family ABI v12 的一次性确认事务契约一致，但仍只记录交互结构，不提交截图或商业内容，也不替代 Release Sandbox/Windows E3。

## 2026-08-30：原生菜单 Skip 的 Headless 复验

- 开发签名 Release CLI 用序列化 secondary-pointer 在剧情中打开 Family API v11 菜单，再以两次方向键和 Enter 选择 Skip。报告通过：674 fixed steps、41 条物理输入、4 个 checkpoint、零 diagnostic。选择前、选择后和继续运行 10 秒后的未读消息 PNG 字节完全一致，与原版空白进度现场观察相符。
- 复验还暴露了 Control wait 的所有权问题。provider 原先在按键状态变化时把活动消息的同一 token 从 `Input` 改成 `Time`，Headless 会按 AwaitQueue 唯一性返回 `ASTRA_EMU_HEADLESS_WAIT_DUPLICATE`。现行实现让启用 `.pragma enable_control` 且允许快进的 message wait 直接声明 `control`；Host 完成原 wait 后，family 才推进消息，不再发布替换 token。未启用 pragma 的消息不会把 Control 当作确认输入。
- Minori 173 项 library tests 全部通过。随后使用独立 launch profile 从空白进度重跑 Control 首路线：首个选择在第 84 tick 出现，结局影片从第 3368 tick 播放到第 15394 tick，路线在第 15635 tick 返回标题，并于第 15636 tick 退出。报告消费 28 条物理输入、呈现 251 帧，自然解锁数为 1，diagnostic 为空。标题、路线和返回标题三个 checkpoint 已人工检查，未见缺字、裁剪、拉伸或明显图层错误。该结果关闭当前身份的 Control 首路线 Headless E2；稀疏采样不构成 120 Hz 性能证据，也不替代 Release Sandbox 或 Windows E3。

## 2026-08-30：WMF Release E2 与原版现场复测

- Windows 默认组合已通过 AstraMedia 的统一 `IncrementalMediaDecoder` 绑定 `astra.decode.wmf.incremental`。开发签名 Release CLI 的影片定向 Headless 运行完成 17371 fixed steps、32 个提交帧和零 diagnostic；影片在第 5251 tick 打开，于第 16387 tick 完成。完整音频非静音且未削波，早段和中段 checkpoint 未见上下颠倒、横向拉伸或边缘裁切。
- 原版程序在新的 title session 中显示未解锁标题页；与当前四路线 clear identity 的 `topMenu2` checkpoint 不是同一状态，不能做像素对照。进入剧情后，右键在指针位置打开原生层级菜单。Auto 能持续推进消息；Skip 在空白进度的未读消息处停止，重新打开菜单也不会给 Auto/Skip 添加勾选。runtime 已据此把持久 Skip 限定到已有 read identity 的消息，Control 的 pragma 快进保持独立。该现场结果确认 v11 的菜单层级和 activity ownership，但不等于 Release Sandbox 验收。
- 原版旧 session 的访问异常可通过结束该 session 后重新启动规避；新 session 尚未到达与 WMF checkpoint 相同的影片时间点。原版同点影片、完整 WMF 路线、正式音频听审、Release Sandbox 和 Windows E3 继续开放。
- `astra-emu-minori-cli` 中已不再编译、但仍留在源码树的旧 GARbro NRBF reader 已删除。CLI 只保留 archive、script 和 media census；AstraEMU CLI 与 Manager 的依赖图均不含 `mlua`，也没有恢复 importer、Luau patch 或明文 cache 路径。

## 2026-08-30：原版影片后端与平台解码诊断

- 原版主程序的 PE 导入和 COM 调用已确认影片链路使用 DirectShow Filter Graph，而不是 Media Foundation：创建 `CLSID_FilterGraph`，查询 `IGraphBuilder`/`IFilterGraph2`，并创建 `CLSID_VideoMixingRenderer`（VMR7）及 `IVMRFilterConfig`、`IVMRWindowlessControl`。`VMR7` 属性配置和关键 COM 调用均检查负 HRESULT 并进入清理/失败分支，没有观察到静默切换后端的路径。
- 同一授权 AVI 经 AstraMedia 的 Windows Media Foundation 全流诊断完成 2106 帧、87.916666 秒，PTS 单调且未返回解码错误；固定 FFmpeg 路径则在同一源中报告 7 个 concealment frame。这个结果说明平台解码器能处理该 authored source，但还不是原版逐帧 parity，也不授权把 FFmpeg 的错误静默吞掉。
- AstraMedia 已增加只读 `IStream` adapter、`open_windows_video_reader` 和 `open_windows_audio_reader`：它们直接持有有界 `Read + Seek + Send` source，并交给 MF byte stream，不复制到 HGLOBAL，也不建立临时文件。公开 MP4 fixture 的视频与音频 reader 回归均通过。授权 AVI 的独立 reader 诊断得到 2106 个单调视频帧，以及 2110 个 PCM chunk、8439808 个交错 sample；两条轨道均到达 EOS。当前完成的是公共分轨 reader seam，统一音视频 packet provider、seek generation 与 launch registry binding 仍未完成。
- AstraMedia 公共 registry 现提供显式 `astra.decode.wmf.incremental`。它从同一个 MF Source Reader 输出统一 timestamped audio/video packet，校验动态 media type、轨道、PTS、generation 和预算，并实现 seek/cancel；未注册、codec 不匹配和平台不可用均由 registry 阻断。公开 MP4 覆盖双轨、seek 和 cancel，授权 AVI 的统一 provider 全流得到与分轨诊断一致的 2106 个视频 packet 和 2110 个音频 packet。
- 后续生产接线仍采用显式 provider binding：Windows 可选择 WMF，FFmpeg 保留为另一项显式选择；二者不构成 fallback chain。完成 Minori composition、Headless movie checkpoint 与原版同点复核后才能替换当前 Release binding。

## 2026-08-30：自然解锁后的鉴赏子页

- 复用从零完成四条路线后留下的同一隔离 writable identity，通过序列化物理输入依次进入 `Memories` 根页、BGM、CG、回想和影片列表。Headless 报告通过：82 fixed steps、20 个呈现帧、9 个 checkpoint、45 个资源、零 diagnostic；没有注入解锁状态，也没有改动用户存档。
- 模型已查看九个保留 checkpoint。标题、鉴赏根页、BGM 列表/选择/播放状态、CG 两页、回想和影片列表未见明显缺字、裁剪、拉伸、错层或残影。该报告证明自然 progress 能驱动当前子页数据和输入链路，不证明影片列表中的资源已逐个播放，也不证明与原版像素一致。
- 原版 Sandbox 的既有 session 被原程序自身的重复异常对话框阻断，无法可靠返回标题或进入鉴赏。本轮没有从崩溃后的画面推断 UI 或媒体容错；原版同点比较、正式 Sandbox 行为验收和 Windows E3 继续开放。

## 2026-08-30：固定 FFmpeg 依赖与 WMV3 对照

- 根目录新增 vcpkg manifest，固定到提供 FFmpeg `8.1.2#3` 的 baseline，并只启用 `avcodec`、`avformat`、`swresample` 和 `swscale`。Windows CI 改为检出同一 baseline 后按 manifest 安装；AstraMedia 的完整与增量 provider 都会校验实际加载的 `libavcodec 62.28.102`，旧版或混装 DLL 直接返回 `ASTRA_FFMPEG_RUNTIME_VERSION`。
- 当前 MSVC/vcpkg build 通过普通文件输入和 AstraMedia custom AVIO 输入解码同一授权 WMV3，均输出 2106 帧，也均复现 7 个 P-frame concealment。由此排除 custom AVIO 读取差异是唯一根因；升级与版本固定提升了可复现性，但没有消除质量警告。
- 外部 FFmpeg 8.1.2 对同一抽样影片没有打印 concealment 数量，但明确报告 7 个 `corrupt decoded frame`，与库路径数量一致。AstraMedia 现在读取 `AVFrame::decode_error_flags`；出现 `FF_DECODE_ERROR_CONCEALMENT_ACTIVE` 或其他 decode error flag 时返回 `ASTRA_FFMPEG_CORRUPT_FRAME`，不再只向 stderr 打印后继续生成零诊断报告。该对照不提交影片、文件名或逐帧内容；在原版同点比较或明确的素材容错契约形成前，WMV3 质量继续 blocking。

## 2026-08-30：隔离进度下的自然解锁链

- 为避免改动用户存档，本轮使用独立 launch profile identity 和独立 writable root，从零开始顺序执行四条真实路线。Sui 报告通过并严格观察到累计解锁数 1；Ren 已发布 `route_complete` 并完成结局影片，但私有输入在返回标题后使用了过时的累计值断言，因此该报告按协议失败。随后 Ayame 的通过报告严格观察到累计解锁数 3，证明 Ren 的原子持久化已被下一 session 读取；Tohka 的通过报告再严格观察到 `route_complete` 和累计解锁数 4。
- 使用同一隔离 identity 新建标题 session 后，标题自然出现 `Memories`，物理方向键和 Enter 可进入鉴赏根页。对应报告通过，15 fixed steps、2 个 checkpoint、零 diagnostic。模型实际查看标题与鉴赏根页，未见缺字、裁剪、拉伸或残留图层。
- 这条链证明四个 clear flag 可由真实路线自然写入，并在新 session 中控制标题和鉴赏入口；它不等于四份路线报告全部为绿色，也不证明 CG、BGM、回想、Movie 子页内容与原版一致。正式门禁仍需在新的空白 identity 下重跑四份无过时断言的独立通过报告，并补齐各鉴赏子页 required checkpoint。
- 未跳过影片的诊断运行确认 FFmpeg/AstraMedia 会保持 media wait，分别在 87.916667 秒与 185.583333 秒的声明时长后提交 owner-side completion。日志同时出现 WMV3 damaged-frame concealment；因此这里只确认 completion/fence，不确认逐帧质量或原版视觉一致性。

## 2026-08-30：原生消息控制标记与音频时长

- IDA 对原程序 `CTextDrawer` 和 `MsgSubCmd` 的静态分析确认：`\\a` 请求自动推进，`\\v` 等待当前语音结束，组合 `\\v\\a` 先等待语音再自动继续；`\\x{...}` 进入内联子命令 parser，已确认的 `load` 形态按延时、角色 slot、PNG 资源、可选 transition 和 opacity 调度。未知控制、截断参数和越界值继续阻断，不从名字推测语义。
- 当前授权样本的脱敏 census 为 18319 条 message、26 条内联 `load`，`\\v` 和 `\\a` 只在同一处组合出现。报告不保存脚本名、正文、角色资源名或 source span；`pos/trans/vis` 仅由原程序 dispatcher 证明存在，没有因样本未使用而注入 runtime。
- runtime state 已硬切到 `astra.emu.minori.runtime_state.v28`。控制序列在进入 backlog、翻译 Hook 和文字 surface 前剥离。`\\v` 通过 AstraMedia 的 seekable Symphonia metadata reader 读取 revision-pinned VFS stream，不分配整段 PCM；缺少可靠 duration 时直接阻断。授权样本定向探针耗时 12 ms，替代了此前会在无缓存压缩 entry 上反复解压的整文件读取。内联 `load` 使用固定时钟和可序列化 pending state；current 与 next 作为两个 retained texture 同时提交，以互补 alpha 交叉淡化，完成后再提升 next。消息输入会走同一清理路径强制完成。该实现已有定向测试，尚未取得 v28 的原版同点视觉证据。
- 使用签名 v28 plugin 和 Release `astra-emu-cli headless` 重跑此前通过的标题启动物理输入。结果完成 5258 fixed steps、83 个采样帧和三个 checkpoint，diagnostic 为空，结局按脚本返回标题。`runtime_step` 最大值为 0.553 秒，旧候选因整文件读取出现的约 4147 秒尖峰没有复现；整次日志跨度约 345 秒。模型查看标题、路线和返回标题三个保留帧，未见新增裁剪、拉伸或图层残影。采样没有命中 inline load 的交叉淡化中间态，因此不能用这次回归代替精确 checkpoint 或原版同点视觉对照。
- AstraMedia WAV fixture、仓库 Ogg 样本和 Minori 170 项 library 回归均通过。该结果是 parser/runtime E1；真实罕见行的 Headless checkpoint、原版同点视觉对照、四路线 E2 和 Windows E3 尚未重跑。

## 2026-08-30：控制标记观察边界与契约纠偏

- 授权原版在既有 Sandbox session 中恢复到普通剧情画面，但先后在消息阶段和选择 Load 菜单时触发访问异常。目标罕见行没有被可靠复现，因此本轮不据崩溃后的画面解释控制标记，也不在 runtime 中增加静默删除或推测语义。
- 全量脚本的私有统计继续只确认该行末组合出现一次；公开记录不保存脚本名、正文或截图。下一步需要可复现的原版断点/反编译证据，或在干净 session 中到达同一 source span 后再修改 message parser。
- 同步清理现行契约中的两处迁移残留：AstraEMU Manager 不再声明通用 Trusted Luau script profile，Minori media 也不再声明 64 MiB 进程内明文 entry cache。现行路径是严格 launch profile、相对 private file、family-owned 流式解密和 AstraMedia custom AVIO。
- PAZ chunk transform 进一步接管 source reader 返回的 owned buffer。Blowfish、RC4 和 movie transform 在该 allocation 原地执行；顺序 stream 只截断有效 stored range 后直接保留，不再先复制 decrypt 输入、再 collect 一份 pending buffer。新增 pointer-identity 回归和原有 7 个流式用例均通过；真实峰值内存规模证据仍未完成。

## 2026-08-30：FFmpeg custom AVIO 与当前 E2 边界

- AstraMedia 的 FFmpeg 增量入口已从明文临时 spool 改为 custom AVIO。Host 把有界 `Read + Seek + Send` reader 的所有权交给 decoder；FFmpeg 通过 64 KiB callback buffer 直接读取和定位 VFS 明文流，不再生成第二份完整明文文件。reader、AVIO context 和 demux context 按显式所有权顺序释放，回调 panic、I/O 错误、越界 seek 和输入预算异常都会阻断。
- 同一接口完成了直接文件与 VFS reader 的逐次 read/seek 对照，真实 AVI 全流解码到 EOS；`astra-media` 的 FFmpeg 定向测试、受影响 `clippy` 和 CLI feature 构建通过。Minori 影片 completion 现在同时要求播放时钟到达 duration 且 decoder 到达 EOS，不能用首帧或时长猜测完成。
- 当前签名 Release identity 的 120 Hz Headless GPU 运行已完成首段真实影片的全流解码与 media fence。随后发现既有私有路线输入在当前脚本节奏下包含大量重复推进，单次运行无法在合理时间内闭合四路线；诊断运行已主动中止，因此没有最终 artifact 或 route-pass report。当前只能记作媒体子链 E2，完整路线、Release Sandbox 视觉验收和正式 Windows E3 仍开放。

## 2026-08-30：packed stream 的声明尺寸边界

- 新 key-file identity 的真实八包 full verify 在 `bg` 中稳定复现一个边界：zlib 完整解压结果比 index 的 `unpacked_size` 多 8 个全零字节。一次性 reader 过去会在验证尾部全零且不超过 16 字节后裁剪，流式迁移遗漏了这项格式语义，因此在声明 EOF 处错误阻断。
- `MinoriDecodedStream` 现在到达声明尺寸后继续读取底层 raw/zlib stream，直到确认真实 EOF；只接受最多 16 字节的全零尾部。出现非零尾部、第 17 字节、zlib 错误或提前 EOF仍返回 `ASTRA_EMU_MINORI_ENTRY_SIZE`，没有放宽 checksum，也没有恢复明文缓存或 fallback。
- 合成回归覆盖跨 64 MiB decrypt chunk 的 zlib checksum、8/16 字节全零尾部、17 字节和非零尾部。通用 full verify 同时改为每个 entry 单次 `open_stream` 顺序读取，再用独立 `read_range` 复读首尾，避免无缓存 packed entry 按块反复从头解压。
- Release reader 随后在当前 key-file/streaming identity 下完成真实八包校验：8 个 source、14502 个 entry、43818 个逻辑读取范围、6624958365 decoded bytes，aggregate hash 为 `sha256:e641854399512fea4182ebc7de845436d37d3eaef0b31d748b41c8bd23f9e64b`。该结果关闭本轮 full verify，不代表峰值内存、四路线 GPU E2、Sandbox 视觉验收或正式 Windows E3 已完成。
- 同一 identity 的脱敏研究工具随后完成 89 个脚本 census：33728 行、33695 条命令、29 个 opcode，unknown opcode 为 0。媒体 census 覆盖 `bg`、`bgm` 和 `mov`：4665 个图像/音频条目、1951 ANI（6723 frames）、9 SQZ（224 frames）、2655 PNG、49 Ogg，以及 5 个 AVI container。它证明生产 reader/adapter 能完整遍历当前素材，不等于影片逐帧播放、音频听审或视觉 parity。

## 2026-08-30：Family API v11 与原版右键菜单（历史）

- 原版现场观察确认：右键打开的是系统菜单，不是 Save 页。标题与剧情阶段的根菜单不同；剧情菜单包含消息框显示、Auto、Skip、Quick Save、Save、Load 和 Config，窗口、Help、Game 子菜单位于其后。转场期间右键不生效。
- 当时 Family API hard cut 到 `astra.emu.family_abi.v11`。Family 通过 `LegacySystemMenuTransactionV1` 非阻塞发布层级、启用状态和勾选状态，Host 只负责显示并回送 `Select` 或 `Dismiss`。错配 menu id、重复 item、无效 parent、不可选 item 和并发菜单都直接阻断；现行 identity 已升级到 v12，并在同一通道增加 confirmation transaction。
- Windows Release CLI 使用 AstraPlatform context-menu provider；Manager 使用同一 transaction 构建 Slint overlay；Headless 只接受物理方向键、确认键和取消键。三条路径不按 Minori item id 自行解释命令。
- Minori 选中后可以切换消息框、Auto/Skip，或打开 Save、Load 和 gameplay Config。菜单活动期间保留底层 message wait，防止确认或取消动作误推进剧情。
- 当前完成的是 ABI、平台和 consumer 的 E1 定向回归。尚未用 v11 build 完成真实 key-file mount、Headless GPU E2 或 Release Sandbox 视觉复测，不能沿用旧 ABI 的 Save 页现场结果。

## 2026-08-29：key file 与流式解密 hard cut

- Minori launch profile 改用安全相对 `key_file`。mount 通过 `astra-emu-family-core` 的有界只读接口读取一次严格 `astra.emu.minori.keys.v1`，后续不监控、不重载，也不保存 key hash。
- PAZ index 仍做有界整块解密；entry 改为按 range 或顺序 stream 从密文 source 解密。packed entry 每次从起点建立 zlib 流并丢弃 offset 前明文，不生成 seek index、明文 cache 或临时文件。
- AstraEMU 的 Luau private profile、decoder callback、patch overlay、plaintext cache、GARbro importer、相关 Manager UI/evidence，以及 `windowed-e2` 命令已经删除。AstraVN/AstraRPG 的 Luau policy 不受影响。
- strict key parser、private-file boundary、Minori/FamilySupport、CLI 和 evidence 的局部回归已经通过。2026-08-30 的后续 Release reader 也完成了新 key-file/streaming identity 的真实八包 full verify；四路线 GPU E2、Release CLI Sandbox 视觉验收和正式 Windows E3 仍未完成。

## 2026-08-28：无音频设备启动复核

- 在授权 Windows Sandbox 中用当前签名 Manager 启动 Minori。Host 的默认输出不可用时，`FamilyAudioService` 选择 `NullAudioLane`，Manager 窗口仍完成初始化，Diagnostics 面板显示 runtime active 且无 blocking diagnostic。
- 该复核只证明无设备启动和 UI 生命周期不再因 WASAPI 失败而退出；null sink 不产生物理 audio meter，且本轮没有 artifact 输出，因此不能关闭正式音频听审或 Windows E3。
- Diagnostics 摘要现显式显示 `audio_endpoint=none|native|null`；这是当前 session 的端点状态提示，不改变 blocking diagnostic，也不把 `null` 提升为物理音频 evidence。

## 2026-08-28：Family API v10 与右键系统菜单（历史）

- Family API 的 ABI hard cut 进入 `astra.emu.family_abi.v10`。`LegacyStepInput` 增加 `LegacySystemMenuRequestV1`，通过 `FfiSystemMenuRequestV1` 进入 ABI wire。request 目前只允许 `Open`，可携带有界 pointer 坐标和独立 sequence，不把右键当作键盘 alias。
- Manager 将 `pointer.secondary` 的 pressed edge 提升为 typed request，并从传给 family 的 input stream 中移除重复的 pressed edge；release edge 仍留在普通 input stream。重复 pressed secondary、pointer 数值异常和 sequence 冲突都返回 `ASTRA_EMU_SYSTEM_MENU_*` diagnostic。
- 当时的实现把右键直接映射到 Save page。2026 年 8 月 30 日的原版现场观察证明这项语义不正确，v11 已删除该路径；本节只保留迁移历史。
- Family API wire round-trip、validation、Manager promotion/duplicate 和 Minori provider 测试已经加入定向测试；该变更没有改变 Layer2D、音频或媒体 provider contract。
- 无物理音频设备时的 `NullAudioLane` 现在也严格绑定输出声道与 chunk 形状，并拒绝错误长度或非有限样本；它只消费经过同一 Kira/resampler 路径的 owned buffer，不把无设备数据当作物理音频 evidence。
- 新增 service 生命周期回归：Host 的 `OpenAudioOutput` 返回明确的 `ProviderUnavailable` 时，`FamilyAudioService::start_with_client` 仍能完成 worker 初始化、处理挂起请求并正常 shutdown；测试同时确认该 session 的 `null_device` 标记保持为真。这样验证的是公开启动/关闭边界，不是物理音频或 Windows E3 证据。
- Support API 另外提供 `has_physical_audible_output()`，把“混音器产生了非静音样本”和“样本到达物理设备”分开；Manager 的 evidence 只使用后者，避免未来调用方误把 null sink 的 meter 当成硬件输出。

## 2026-08-28：global progress 的新 session 装载

- 新增 provider 级回归：第一 session 通过显式 `astra.provider.storage = astra.writable_file.v1` 自然写入 `REN_CLEAR`，第二个全新 session 在执行入口脚本前先从同一 writable-file port 装载 progress；脚本只在缺少该标记时设置 `SUI_CLEAR`，因此测试可以区分“先装载”与“本 tick 执行后才写入”。
- 回归同时检查 `minori.gallery_unlock_count`、global variable 和持久化 unlock 数量，覆盖真实 provider open→step→shutdown 生命周期。它是脱敏的 E1 控制流证据，不代表四路线自然解锁或完整鉴赏页已经通过。

Windows Sandbox E3 现场检查因 WASAPI 默认输出不可用而无法启动 native audio，随后 prewarm 未收敛并返回 `ASTRA_EMU_NATIVE_PREWARM_DID_NOT_CONVERGE`。随后以显式关闭音频的 direct native run 复核了真实 Minori 画面、消息层、转场/黑场与影片返回路径，并正常结束 `windowed-e2`；这只提供视觉现场证据，不满足音频 meter、artifact manifest 和可回收 machine-readable report 的 E3 门禁。Manager 现在把 `audio_null_device=true` 写入受限 meter 事件，并将 null sink 的非静音数据排除出 `audio_non_silent`，避免无设备运行被误当成物理音频证据。共享目录为只读，未回收可发布 artifact，因此 Windows E3 仍保持 blocking。

## 2026-08-28：真实八包 cache second-run

- 在同一授权八包、同一 mount profile 和同一 private-profile identity 下连续执行两次 `astra-emu-cli vfs ... verify`。两次均覆盖 8 个 source、14,502 个 entry、43,818 次 bounded range read、6,624,958,365 个 decoded bytes，aggregate hash 均为 `sha256:e641854399512fea4182ebc7de845436d37d3eaef0b31d748b41c8bd23f9e64b`。
- 首轮报告 `cache_hit_count=29,648`，第二轮报告 `cache_hit_count=43,594`；第二轮完整读取与首尾复读均命中既有 plaintext identity。输出仅保留计数和聚合 hash，原始 profile、cache、key 与 decoded 内容继续留在 ignored 私有目录。
- 这项关闭真实跨运行 cache second-run；cache identity 漂移、损坏恢复和配额淘汰仍由 support 单测/独立门禁约束，不能用本轮命中数替代。

## 2026-08-27：FFmpeg 构建身份校正

- Minori 动态插件 descriptor 现在从 Cargo build script 实际提供的 `CARGO_FEATURE_*` 变量收集可选 feature，并以 `CARGO_CFG_FEATURE` 作为补充后规范化去重，避免不同 Cargo 环境下 descriptor 与 `cdylib` 的 feature 描述漂移；当前 stable Release 构建已核对启用 `ffmpeg-vcpkg` 时两者身份一致。
- 该项只修复构建/签名边界，不改变 decoder 选择：Minori 仍必须显式绑定 AstraMedia `ffmpeg-vcpkg`，缺 provider 或运行时库直接 blocking。真实八包 Headless 仍需继续完成四路线、鉴赏、正式音频听审和 Windows E3 验收。

## 2026 年 8 月 27 日：Title route 重新进入

- 修复一条 route 在 Title launch 中完成后再次按下 New Game 会继续使用已到达脚本末尾的 VM 状态的问题。session 现在保留经验证的初始 entry URI；Title 的 `StartGame` 先重新读取该脚本，gallery 仍使用自己的显式 script replacement。
- provider 级脱敏回归通过四个物理选择依次执行四个 route，检查 clear flag 的自然累计、标题变体 0→1→2、每个 `.end` 返回 Title 且 session 不 terminal。它只覆盖 cross-module 控制流 E1，不替代真实四路线 Headless、完整鉴赏解锁、正式音频听审或 Windows E3。

## 2026-08-27：路线 choice 与自然 clear 状态

- Minori VM 新增四分支路线回归，采用授权样本 `K06_01` 已确认的 choice → tail `chain` 结构。脱敏 fixture 逐项选择四个分支并执行对应 clear script，验证 global clear flag 跨脚本保留、标题变体按已确认规则从 0→1→2 变化；Title launch 下 `.end` 回到标题而不是终止 session。该测试固定控制流语义，仍不替代真实四路线自然运行、完整鉴赏解锁和 Windows E3。

- 同日补充 `K06_01` gate 回归：缺少 `REN/SUI/AYAME` 任一前置 clear 时只发布三项选择；三项满足且 `D06` 未置位时按原观察 tail-chain 到 `K06_05.sc`。测试只读取已持久化的 global state，不注入隐藏解锁，也不把不可达第四项当作可选项；真实多路线执行和全量鉴赏证据仍开放。

- 脚本装载边界同步收紧：初始 `open`、`probe`、全量资源审计与 chain 目标统一使用 bounded `.include` 展开和循环检测，并保留根脚本 source identity。新增回归证明初始入口不会把 `.include` 当作未知 opcode；缺失引用仍直接 blocking。

## 2026 年 8 月 27 日：Family VFS 单文件导出边界

- FamilySupport 新增与整树导出共用约束的单 entry 原子导出：先验证 manifest entry、相对路径、容量和目标目录项，再以 owner-only 临时文件按 4 MiB range 流式读取，完成同步后提交到目标文件。
- Manager 的 VFS 导出现在只接受当前 family mount，并通过该 helper 写入；既有文件（包含符号链接）、非法 selector、取消、短读和中途失败都会阻断并清理暂存，不再使用无界 `std::fs::write` 或暴露本地目标路径。该项仍不构成 macOS extract、Linux FUSE 或 Windows E3 证据。
- 通用 `vfs read --output` 也复用同一类 FamilySupport 私有原子写入 helper，统一 64 MiB 上限、符号链接拒绝、owner-only 权限和失败清理；默认 report 模式与显式 hex/text stdout 模式保持互斥。
- 输出父路径按组件检查符号链接和元数据错误，避免通过祖先 symlink 越界；这只收紧宿主写入安全边界，不改变 VFS 内容。
- CLI 将私有 writer 的内部错误映射回 `ASTRA_EMU_VFS_READ_OUTPUT_*`，因此命令行诊断仍保持 VFS 公共命名空间，不泄露 support/provider 实现细节。
- Linux 只读 FUSE 的 range read 对底层短读改为返回 `EIO`，不再把不完整文件内容交给挂载点；真实 Linux FUSE 运行证据仍未形成。

## 2026 年 8 月 27 日：缓存私有目录边界

- `PlaintextCache` 枚举缓存根目录时拒绝所有符号链接，并将其视为 `ASTRA_EMU_MINORI_CACHE_CORRUPT`。这样查找不会跟随目录外目标，也不会修改无关文件权限；Minori 重新挂载后对被篡改缓存保持阻断，不会重新解密或使用 fallback。
- FamilySupport 还增加了跨实例 cache 回归：释放首个 cache 实例后，第二个实例从同一私有根目录验证并读取 envelope；private-profile identity 变化只产生 miss。该项是公共 cache contract 证据，不替代真实八包的第二次 full verify、容量淘汰和跨运行 volume evidence。

- system/gallery 回归现在实际启动已验证的 `fb_ren_04.sc`：从 Title 进入 Memories 和 Replay，依次消费 transition、stage、audio、wait 边界，完成 `.end` 后回到 Title；这保持 direct-entry 的 terminal 语义不变。测试不注入 unlock，也不把该控制流证据扩展为四路线自然解锁、movie gallery 视觉 parity 或 Windows E3。
- movie gallery 另有 provider 级回归：`fb_aya_12.sc` 在显式 VFS movie resource 上发布唯一 `LegacyVideoCommandV1::Play` 与 media fence，消费完成结果后按 `.end` 回到 Title；资源不可用仍是 blocking。它只覆盖脚本/ fence 生命周期，不宣称真实 FFmpeg 影片逐帧 parity。

## 2026 年 8 月 27 日：当前 ABI v9 direct-entry 终点复验

- 在重装后的 stable 工具链上重新构建并签名 FFmpeg profile，当前样本的 A01 direct-entry 终点 slice 通过：3,084 fixed steps、5 个提交/栅格化帧、2,388,480 个音频帧、无 diagnostic，并到达 `terminal=true`。该运行没有保留正文、截图或媒体 payload，也没有把短程终点误记为首路线全流程、自然解锁、正式音频听审或 Windows E3 证据。

## 2026 年 8 月 27 日：AstraMedia 增量 provider 实际接线复验

- Manager 与 Headless CLI 已删除 `MinoriAviDecoder` family wrapper；两者通过 AstraMedia 的 `open_ffmpeg_incremental_reader` 显式绑定 `astra.decode.ffmpeg.incremental`，直接把有界 VFS reader 交给 `IncrementalMediaPlayback`。AstraMedia 负责 FFmpeg demux/codec、PTS、PCM、预算和取消，Minori 只保留 AVI 身份约束。
- 同一当前样本的 direct-entry media slice 通过：3,084 fixed steps、2,697 个呈现帧、2,388,480 个音频帧、1 个 route checkpoint、`terminal=true`、诊断为空。FFmpeg 的 WMV3 packet 由 AstraMedia 逐步解码，未重新引入家族手写 decoder、平台 fallback 或整片预解码。
- 该结果只验证新公共接线和实际影片流，不关闭完整路线、自然解锁、正式 WAV 听审、第二次 cache、Linux FUSE、macOS extract、Manager 实机预览或 Windows E3。报告和媒体文件继续留在 ignored 私有目录。

## 2026 年 8 月 27 日：PAZ 明文范围读取缓存

- `MinoriMountedVfs::decoded_entry` 在完成 source mutation、encrypted hash、解密和明文尺寸校验后，保留一个有界的进程内明文 entry（最大 64 MiB）。同一 entry 的后续 4 MiB range 不再重复从磁盘 cache 读取完整明文；identity 变化会立即替换缓存，超过上限的 entry 不驻留进程内。
- 该缓存只改善顺序 range read 的 I/O 分配行为，磁盘 cache 仍使用既有 identity、完整性校验、原子写入和配额；不会进入 manifest、save、report 或日志，也不改变 movie 的 source-backed range transform。
- 新增合成回归覆盖首次解密与后续 range 命中的 `cache_hit` 语义。真实八包启用 cache 的 full verify 已完成：8 个 source、14,502 个 entry、43,818 次 range read、6,624,958,365 decoded bytes，`cache_hit_count=43,594`，aggregate hash 为 `sha256:e641854399512fea4182ebc7de845436d37d3eaef0b31d748b41c8bd23f9e64b`。该结果只记录脱敏计数和聚合 hash；更换 identity、淘汰、损坏恢复与跨运行 cache identity 仍需单独验证。

## 2026 年 8 月 27 日（AstraMedia 输出缓冲）

- `IncrementalMediaPlayback::drain_ready_outputs` 允许宿主复用 output buffer；Manager 与 Minori CLI 不再在每个 presentation tick 分配新的输出列表。视频帧与 PCM chunk 仍按 `(PTS, track_order)` 稳定排序并转移所有权，`take_ready_outputs` 仅保留为一次性分配的便利包装。

## 2026 年 8 月 27 日

### AstraMedia 增量游标边界复核

- `IncrementalMediaPlayback` 现在在打开时校验完整的 `MediaPlaybackConfig`，每次推进都检查单调时钟和 `max_tick_us`，并拒绝与轨道声明不一致的 packet。视频、音频 packet 还会校验资源标识、尺寸、帧时长、声道、采样率、交错样本和 declared duration。
- `max_video_frames`、`max_audio_packets`、`max_video_lead_us`、`max_video_lag_us` 和 `late_video_policy` 已进入同一游标边界。迟到帧在 `Block` 下返回 `ASTRA_MEDIA_INCREMENTAL_AV_SYNC_LATE`，在显式 `Drop` 下计入 `dropped_video_packets`；没有隐式丢帧或 provider 切换。
- 新增的边界测试覆盖 tick 跳变、轨道错配、packet/音频队列预算、非法配置以及迟到帧的 block/drop 两种策略。FFmpeg 和 Minori 测试继续使用相同的共享游标。
- `take_ready_outputs` 现在是宿主适配器的统一所有权边界：它将当前帧与待提交 PCM 按 PTS 稳定排序后移动给调用方，避免 Manager 与 CLI 各自实现 packet 排序和重复“新帧”判断。新增测试确认批次消费后游标不重复发出同一帧，下一次推进只转移真正的新输出。
- FFmpeg 的增量与首帧/整段 provider demux 都改为直接调用 `Packet::read`，不再使用会吞掉非 EOF 错误的 `Input::packets()` 迭代器。截断或损坏的媒体现在保留底层错误并返回结构化 diagnostic，不会静默变成正常 EOF；FFmpeg stream 的真实样本回归仍为 9/9，decode-provider 回归为 14/14。
- Manager 将公共游标输出从微秒转换到毫秒时间轴后，仍按 `(PTS, track_order)` 合并 pending 队列，视频在相同 PTS 下先于音频；新增 binary 回归覆盖该稳定顺序，避免不稳定的同时间戳排序改变呈现/音频边沿。

### 当前 FFmpeg 真实样本 media slice

- 重新构建并签名当前 `ffmpeg-vcpkg` dynamic plugin 后，标题启动、配置页、影片播放、Control 跳过、影片 completion 和返回标题使用同一组 build、profile、mount 与输入身份执行。运行报告为 `passed`，完成 3102 个 fixed step、9 个 retained frame sample，诊断为空；影片 command、停止 command 和 owner-side completion 分别出现在连续的 3098、3099、3100 步，标题观察在 3101 步命中。
- 这次运行确认 FFmpeg demux/codec、AstraMedia incremental cursor、媒体 fence 和 Minori 标题恢复接线可用。它没有到达剧情 terminal，也没有证明完整路线、gallery unlock、正式音频听审、第二次 cache 命中、Linux FUSE、macOS extract 或 Windows E3。

### 当前 FFmpeg 首路线重验

- 针对当前 v9 observation contract 重新生成物理输入后，签名 release plugin 以同一 `ffmpeg-vcpkg` 增量媒体绑定完成首路线。运行报告为 `passed`：3,034,309 个 fixed step、16,150 条物理输入、53 个 retained frame sample、31 个 checkpoint，抵达 route terminal，`route_complete` 与自然 unlock count=1 均命中，诊断为空。
- 该运行覆盖标题、Config、backlog、真实影片 fence/completion、剧情演出、结局返回标题和最终 Exit。影片播放期间的 WMV3 解码仍由 FFmpeg 负责，未恢复 Minori 私有 decoder；报告只保存 identity、计数和 hash，不包含正文或媒体 payload。
- 首路线的通用 await 曾使用已删除的 observation hash 形式，现已改为 v9 的 typed `exists` observation；同时删除了过时的首 choice 等待点。当前报告因此证明路线 terminal 和 post-choice continuation，但不把该次输入写成显式首 choice 视觉 checkpoint。独立 choice slice 仍用于 choice 语义验证。
- 这轮仍不关闭四条自然路线、第四条路线后的完整 Memories/CG/BGM/回想、正式 WAV 逐段听审、cache second-run、Linux FUSE、macOS extract、Manager 实机预览或 Windows E3。模型和人工 review 也不能覆盖这些自动门禁。

### 本轮验证

- `astra-media` library 9/9、FFmpeg stream 9/9、`astra-emu-minori`（含和不含 `ffmpeg-vcpkg`）各 148/148 通过；`astra-media`、`astra-emu-minori`、`astra-emu-cli` 和 `astra-emu-manager` 的增量 `clippy -D warnings` 通过。
- 普通 Cargo release 构建、动态 plugin 签名、Headless media slice、`cargo fmt --check` 和文档检查通过。完整 workspace clippy/test 仍按仓库既有门禁单独处理，不能由这些聚焦结果替代。

### Family trace 与输入边界补充

- Manager core 现在按 family id 生成 VM coverage id，并对 family id 做稳定字符校验；Minori 与 FVP 的 coverage 不再共享 `fvp.vm.*` 命名空间。provider step、wait、choice 的日志只保留 step、PC、计数、timer 和 identity，不写正文、资源 payload、key 或本地路径。
- Headless 的 F5/F9 物理按键边沿固定映射为 `function:5`/`function:9`，未绑定按键仍被拒绝。该修正通过 Minori provider、Manager core 和 CLI 的增量回归测试。

## 2026-08-26

### Retained Layer2D composite cache复验

- 在不改变 FFmpeg provider、mount profile、物理输入或 sample cadence 的前提下，Headless CLI 的 retained `Layer2D` CPU composite 增加了按 viewport、完整 layer state（含 surface generation）和已验证 base-frame 尺寸的缓存命中。缓存只跳过重复 surface read/composite，仍提交 presentation edge、wait semantics 和 frame sample；状态或尺寸不完全一致时仍走原始全量合成。当前签名 release 复验完成 139 个 fixed tick、5 个 sample、638 次 bounded VFS read、约 99 MiB、零 diagnostic，`step_total` 约 35.9 s，`effect_dispatch` 约 22.4 s，`media` 约 48 ms；`movie_60` 仍人工查看通过。该优化只形成性能诊断证据，未宣称完整路线或正式 120 Hz gate。

### 增量视频播放迁移到 AstraMedia

- Minori 的 `.avi` 运行时现在只保留 RIFF/AVI 身份检查和 family provider binding。增量解码生命周期由 `astra-media::IncrementalMediaDecoder` / `IncrementalMediaPlayback` 管理，统一处理 PTS read-ahead、BGRA 帧边界、PCM 转换、pending audio 限额、seek/cancel 和 codec-neutral telemetry。
- 本节最初使用私有临时 spool 连接 reader 与 FFmpeg；2026-08-30 已由 custom AVIO 取代。现行路径由 FFmpeg 直接回调有界 `Read + Seek + Send` reader，不生成第二份完整明文文件。Minori 的 Manager、CLI Headless 和 preview 均绑定同一 `ffmpeg-vcpkg` provider；FFmpeg feature 未启用或 probe/decode 失败时返回稳定 blocking diagnostic，不调用 WMF、平台 codec、RFVP 或手写 decoder。
- Manager 的 Minori host cursor 也已切换到 `IncrementalMediaPlayback`，只把当前时间窗内的新帧和有界 PCM chunk转移到通用 timeline，避免再次复制完整 decoded movie；`take_current_frame` 用所有权移动保持跨层零拷贝边界。CLI/native/headless composition 现在都把 video provider 作为显式 launch binding，Minori 在 `disabled` 或未知 binding 下直接阻断。
- 已删除 CLI 的 `avi_range` 解码路径以及 Minori 对 `na_mpeg2_decoder`/`wmv-decoder` 的生产依赖；RFVP 自身的 transitive decoder 仍属于 FVP provider 边界，不由 Minori 复用或改写。该项已通过 `astra-media` 增量 cursor 单元测试、`astra-emu-minori` focused tests 和 Manager/CLI 增量编译；真实影片 FFmpeg feature run 与 Windows E3 仍未形成公开证据。
- 当前授权样本的五个 `.avi` 均确认是 `AVI/WMV3/PCM s16le/48 kHz/stereo`。FFmpeg feature 的真实 Headless media slice 已完成：60 个固定 tick、16 个呈现帧、111104 个音频帧、音频非静音、`diagnostic_codes` 为空；`movie_60` checkpoint 的渲染 hash 与此前参考运行一致。该证据只说明增量 provider、VFS range、音频队列和 Headless artifact 接线可用，不等同 Windows E3 或完整路线通过。
- rebase 到最新 `master` 后重新编译并签名动态 Minori plugin，使用同一 FFmpeg 绑定重新执行上述真实样本 slice：139 个固定 tick、5 个采样帧、638 次 VFS read、约 99 MiB 有界读取、约 81.9 s 总 step time，`diagnostic_codes` 为空；`title_initial`、`config` 和 `movie_60` 均成功生成。当前 `movie_60` 画面已人工查看，影片比例、日文正文、透明叠加和层次正常；软件 WMV3 解码仍是主要耗时，不能把该 E2 结果写成性能门禁或 Windows E3 通过。

## 2026-08-25

### Rust 1.98 trust-root 复核与当前 v9 短程 smoke

- 在重新安装 stable Rust 1.98 后，重新编译并签名当前 Minori dynamic plugin；CLI 在编译期嵌入同一 development signer 与 family public-key trust root，随后用当前 v9 `Native + MultiLayer` composition 运行直接入口短程。Headless report 为 `passed`，执行 431 个 fixed step、消费 27 条物理输入，提交并栅格化 13 个 frame，产出 344576 个 audio frame，无 diagnostic，标题与场景 frame hash 不同。
- 这次运行按设计未到达 route terminal，只能作为当前 ABI、surface、Layer2D、CosmicText、图像/音频绑定和基本输入消费的 E2 smoke；它不关闭四条自然路线、gallery unlock、正式 audio review、movie gallery 原版视觉 parity、cache second-run、Linux FUSE、macOS extract 或 Windows E3。
- 旧 gallery JSONL 序列在当前 identity 重放时触发 `ASTRA_EMU_HEADLESS_CHECKPOINT_AFTER_TERMINAL`，因此不再沿用历史 gallery report。需要重新生成与当前 mount/profile/global-progress identity 一致的物理输入；运行时保持 fail-fast，不通过 fallback 或注入 unlock 修复该证据缺口。

### Minori video codec gate

- Manager preview and playback now reject every Minori video extension except the
  explicitly bound `avi` codec before entering the FVP compatibility table or
  Windows Media Foundation path. The Headless driver applies the same strict
  extension check before opening its range-backed AVI decoder. This closes a
  family-boundary hole where a future `.wmv`/`.mp4` entry could otherwise be
  interpreted by a provider belonging to another family; the diagnostic is
  `ASTRA_EMU_MINORI_VIDEO_CODEC_UNSUPPORTED`.
- The guard is pure Rust and covered by the Manager focused test. It does not
  claim movie-container parity or Windows E3 evidence; those remain separate
  gates.

### Fresh v9 Headless smoke after toolchain reinstall

- The current signed Minori plugin was rebuilt with the repaired stable Rust
  toolchain and launched through the normal `--family minori` composition. A
  bounded title-to-scene input sequence completed 431 fixed steps, consumed 27
  physical input messages, produced 13 submitted/rasterized frames and
  344,576 audio frames, with `status=passed`, no diagnostic, a non-silent WAV,
  and distinct title/scene frame hashes.
- Manual inspection of the two retained checkpoints found the expected title
  layout, Japanese glyphs, scene composition, alpha and aspect ratio. The
  sequence intentionally stops before a route terminal, so this is a fresh
  current-v9 smoke/E2 result, not full-route, gallery-unlock or Windows E3
  evidence. The artifacts remain local-private.

### Family media preview binding

- Manager family VFS audio entries now use the explicitly bound pure-Rust `astra.decode.symphonia` provider; it is a packaged platform provider rather than a declared fallback. The UI receives only codec, sample-rate, channel, frame-count and duration metadata, never PCM or a provider handle.
- Video preview remains a separate binding: Windows may select the explicit Media Foundation provider, while unsupported targets return a stable unbound diagnostic. ANI/SQZ now have a separate family-owned first-frame binding; animation playback remains a runtime concern.
- The Windows binding now declares `avi` explicitly for RIFF/AVI Minori movies; this removes only the codec-identity block. An actual Windows preview, Headless, or E3 run is still required before movie support can be marked verified.
- ANI/SQZ preview now uses the explicit family-owned `astra.decode.minori.image` binding and returns only a bounded first-frame RGBA8 buffer. It does not imply animation playback or original pixel-parity evidence.
- The Headless CLI runtime driver now registers the same family provider and selects `astra.decode.minori.image` for ANI/SQZ resource scenes. Standard image codecs continue to use `astra.decode.image`; no registration-order or codec fallback is introduced. A runtime route containing ANI/SQZ still requires real media evidence before animation playback can be called complete.
- The CLI validates the provider's explicit `rgba8:first_frame:WxH` format against the resource descriptor before handing pixels to the retained renderer; a generic `rgba8` result is not accepted for the Minori family binding.
- The Minori runtime resource resolver now recognizes ANI/SQZ metadata through the same strict container adapters instead of sending those resources through `image::ImageReader`. It emits the family codec and verified first-frame dimensions to the host; multi-frame playback is still intentionally open.
- The v9 `Layer2D` texture path now sends both standard image resources and ANI/SQZ resources through an explicit `DecodeProviderRegistry` binding. Standard images retain encoded-format identity checks; ANI/SQZ require the family provider's first-frame dimensions and output contract before Renderer2D upload.
- The Manager's legacy `RuntimeLiveResourceScene` path now uses the same explicit image registry and codec identity checks. Minori ANI/SQZ never pass through generic `image::load_from_memory`; unsupported codecs, provider identity mismatches, malformed first-frame metadata, and decoded byte-size mismatches stop the transaction. This closes the host-side decode bypass for the retained resource-scene path; it does not add multi-frame animation playback.
- Minori AVI playback now uses the shared AstraMedia incremental contract in both Headless and Manager. Each host registers the explicit `astra.decode.ffmpeg.incremental` provider, passes a bounded VFS reader, and consumes timestamped video/audio packets through `IncrementalMediaPlayback`; Minori only enforces the AVI family extension and RIFF/AVI container identity. Manager no longer routes Minori `.avi` through the FVP compatibility table or a platform codec; unsupported containers and missing FFmpeg bindings remain blocking.
- 该历史 preview 绑定已在 2026-08-26 媒体迁移中替换：Manager VFS movie preview 与 timestamped playback 现在共同绑定 AstraMedia `ffmpeg-vcpkg` 的有界增量 provider；不再使用 `astra.decode.minori.avi` 纯 Rust decoder。其他 family 的视频仍使用各自显式 provider，缺失时保持 blocking。

### Manager family selection is explicit at startup

- Manager startup now constructs only the pure-Rust static Minori provider; the external FVP provider is loaded only after an explicit `fvp` family selection and is rebuilt before the session opens. This keeps composition-root provider choice explicit and avoids loading an unselected native family.
- The same-family path is rebuilt when a real mount is selected, because the initial idle provider is intentionally bound to the desktop VFS while a Minori launch must bind the decrypted family VFS adapter. Manager focused tests and clippy pass after this change.

### Family VFS 图片预览的显式绑定

- Manager family-mounted VFS 的 PNG/JPEG/BMP/WebP 预览现在先用 bounded range read，再通过 `astra-media` 的 `DecodeProviderRegistry` 绑定 `astra.decode.image`；未绑定 codec、尺寸超限、解码输出格式或长度不符时只显示稳定 diagnostic 与有界 hex，不把 raw bytes 当作图片，也不走系统 codec fallback。
- 解码后的 RGBA8 buffer 在 UI 线程创建 Slint `Image`，VFS model 不携带路径或 native handle。桌面目录仍使用原有显式 resolve path；Minori mount 使用内存 buffer，`.ani/.sqz` 等专有容器没有匹配 provider 时保持阻断并可查看 hex。
- Manager 8 个 focused tests、UI contract test、clippy 和文档检查通过；音频/视频 preview provider binding 和正式视觉审查仍开放。

### Manager family mount 与 Minori runtime 接线

- Manager launch 现在根据显式 family override 或 `scr.paz` entry 选择 `fvp`/`minori`；Minori 通过游戏目录内的 `astraemu.minori.mount.yaml` 调用静态 `MinoriVfsFamilyFactory`，再把 `LegacyMountedVfsReaderAdapter` 绑定到 ABI v9 host。RuntimeBridge 在无活动 session 时销毁旧 family instance，并重建同一 ABI v9 provider，不按注册顺序或隐式 fallback 选择。
- Manager VFS tree/文本 preview 在 Minori mount 生效时读取解密后的 manifest URI 与 range，不再把 archive 原文件误当成脚本；图片、音频和影片仍停在显式 `DecodeProviderRegistry` binding 之前，不以 hex 视图冒充 media preview。
- 这关闭了 Manager 的 Minori family mount、runtime provider 和解密脚本读取接线；完整媒体预览、Manager 端 cache evidence、四条路线和 Windows E3 仍未关闭。没有把 key、商业文本、截图或本地绝对路径写入仓库。

### Manager VFS 文本预览编码选择

- Manager 的 VFS preview 现在在有界读取后先识别 UTF-8/UTF-16 BOM，再对 `.sc`、文本配置和脚本扩展名严格尝试 CP932；解码包含 NUL 或出现替换错误时保持 binary/hex 视图，不把任意二进制静默当成文本。UI 显示实际选用的编码，图像路径和 media provider binding 未被改写。
- 这只补齐 Manager 文本预览的编码检测子路径。图片、音频和影片仍必须通过显式 `DecodeProviderRegistry` binding；Manager 的内部 mount viewer 接线和真实 media preview evidence 仍保持开放。
- `astra-emu-manager` 的 7 项 manager tests、`astra-emu-manager-ui-slint` contract test、clippy 和格式检查通过；没有新增商业资源、路径或文本到仓库。

### Config 跨 session 持久化

- 复核原程序的配置加载/写回字段后，补上 Minori runtime 的 installation-scoped config store。显式绑定 `astra.provider.storage = astra.writable_file.v1` 时，family 在 open 阶段读取 `astra.emu.minori.config.v1`；文件缺失使用原程序默认值，schema、大小、case/package/profile identity 或 postcard 内容不匹配均直接阻断。
- Apply 后只在已应用值发生变化时写入相对 writable-file root，使用 bounded temporary file、保留式 range write、length 校验和 atomic replace。配置 envelope 不含脚本正文、资源、密钥或宿主路径；没有 writable-file binding 的纯 VFS 单元 provider 不会伪造持久化。
- Save slot restore 明确保留当前 installation-scoped config，旧 gameplay slot 不再回滚音量、文字阴影或 play-mode 偏好。新增 round-trip 与 identity-drift 回归；真实桌面跨进程复验和原版全屏/逐字速度行为仍是独立开放项。
- 复核 v9 media continuation 边界：`LegacyAudioCommandV1::Play` 与 `LegacyVideoCommandV1::Play` 没有 seek 起点。snapshot 虽保存活动资源和 continuation marker，restore 只能重新校验资源并从起点提交公共 `Play`，不能宣称原位置续播。该限制已同步到 presentation、script execution 与 coverage 说明，待 ABI/Host seek contract 和真实 evidence 后再关闭；没有引入平台播放器、私有 decoder 或伪造 fence。

### 后台进度与窗口焦点

- Minori provider 现在把已提交的 `progress_in_background` 配置作为有界 `minori.progress_in_background` blackboard observation 发布。默认值 `false` 不制造首 tick 噪声；持久化为 `true`、或从 `true` 切回 `false` 时才提交边沿，load/restore 会重新建立该观察值。
- Windows native host 只对 `minori` family 消费该 observation：失焦且配置关闭时暂停固定 tick 和音频，重新获得焦点恢复；配置打开时继续运行。其他 family 保持原有焦点处理，不共享 Minori 配置语义。缺失 observation 按关闭处理，非 `true`/`false` 值返回 `ASTRA_EMU_MINORI_PROGRESS_BACKGROUND_OBSERVATION_INVALID`。
- provider 与 CLI focused tests 已覆盖 observation edge、缺省和非法值；真实窗口失焦/恢复及完整路线仍需要 Windows E3 evidence，不能由该单元测试替代。

## 2026-08-23

### Family ABI v9 hard cut

- 当前分支已 rebase 到 Family ABI v9 基线 `635527831e89e5ff9b87ac165b5b5532e28356c6`；其中包含 writable zero-copy surface、typed filter graph 以及 Rust 1.98 workspace gate 修正。当前 consumer 提交为 `d153f06e1`。Product Runtime Provider 使用 ABI v4；Minori 的唯一合法组合是 `Native + MultiLayer`，画面通过 Host-owned surface 和 retained `Layer2D` transaction 提交。
- v9 删除了旧 scene transaction、family snapshot、ephemeral text、session resource presentation 和 step budget。旧接口不保留兼容层，也不会在缺少 surface、Hook、字体、decode 或 writable-file provider 时回退。
- Minori dylib 已改用公共 `FfiLegacyFamilyHostAdapter`，不再维护私有 FFI host adapter；surface lease 采用独占、可写、零拷贝 owner，禁止 immutable buffer、复制回写和 const-cast。旧 save/restore/text/resource 导出已从 root module 删除。
- 全局进度已迁到同步 writable-file port，并使用相对路径、临时文件和 atomic replace；旧 provider-result payload 不再进入生产 step。Windows 复验发现 writable root 曾直接使用带 `sha256:` 前缀的显示字符串，冒号会生成非法目录名；现在目录组件固定为原始 digest 的 64 位小写十六进制。Runtime control action 同时补齐实际 Blackboard 写入的 `ActionAccess` 声明，两个问题都有定向回归。
- 当前签名 v9 plugin 已重新挂载 8 个逻辑 archive、14502 个 entry，并完成一次 42 条物理输入的 Headless slice：154 个提交/栅格帧、134144 个音频 frame、零 runtime diagnostic。该运行未到 terminal；6 个 checkpoint 的标签与实际画面阶段没有完全对齐，首张是黑色过渡帧，后续能看到标题、Config 背景、正文和场景变化。因此自动 lifecycle 通过，但视觉审查仍为阻断，不能把它写成完整路线 E2。
- 该复验还暴露了两个 retained-state 根因：Layer2D transaction sequence 过去只看单 tick effect，跨 tick 会重复；文字 surface 每帧重建 glyph resource owner，第二次渲染会重复创建 retained texture。现在 session 持有单调 layer sequence 与长期 `TextRenderResourceOwner`，并增加跨 tick 和重复日文帧回归。Headless 标准报告也改为保存每个 checkpoint 的实际 RGBA observation hash，不再给所有 checkpoint 复用 artifact manifest hash。
- 下文所有 ABI v8 真实样本结果只保留为迁移前行为与回归基线，不能直接证明 v9 完整路线、性能或平台 E3。
- 相邻的 FVP consumer 已按 `f4f64a5bb726c1759350a666a35e0a454b810f61` 删除退役 facade，FVP 固定为 `Ported + SingleLayer`。这不构成 Minori 的性能 E2 或 Windows Manager E3。
- Layer filter 已从 string binding 硬切为 ABI-owned typed graph，并在 Family API、Manager Product boundary 与生成 schema 中保持 node、target、parameter 结构。Minori 当前不提交 filter graph；Host 不会按字符串或 hash 隐式选择图。

## 2026-08-22

### 持久 Auto 与活动消息等待重绑定

- 真实完整路线暴露了一个状态机根因：消息已经建立物理输入等待后，再从原版游戏菜单切换到 Auto，只改变 `play_mode`，不会改变已发布的等待类型。短程测试之所以能推进，是因为它在下一条消息创建时才看到 Auto；持久运行则停在原来的输入等待。这不是 tick 预算或输入序列问题。
- VM 现在把活动消息等待视为同一 token 的 modality。Normal 切到 Auto 时，等待由 `Input` 重绑定为 `Time`；Auto 切回 Normal 时反向重绑定。CLI 与 Manager Host 只接受同 token 的 `Input`/`Time` 互换，其他重复 token 仍返回稳定 blocking diagnostic。Manager Core 的 `RuntimeWorld` mirror 复用已有 `AwaitTokenId`，不生成第二个权威 token。
- Config 的 Auto 速度值 `0` 仍保留原始设置值，但公共 time wait 必须为正，因此运行时把它映射为一个 10 ms timing unit。公共 Await contract 没有放宽，也没有加入即时完成或 tick 跳过 fallback。定向测试覆盖双向重绑定、snapshot、重复 token 阻断和最快 Auto 映射。
- 当前签名 Release plugin 已在真实八包上用持久 Auto 跑完攻略首条路线。输入序列先把 Auto 速度调到 `0`，再用原版菜单物理 pointer 开启 Auto；正文阶段没有周期性 Enter，只在严格观察到 choice active 后提交一次确认。运行推进 25552 fixed steps，提交并栅格化 25557 个 GPU frame，消费 50 条物理输入，最终到达 terminal；snapshot round-trip 成立，coverage hash 与既有完整路线一致，diagnostic 为空。
- 自动音频记录 20441600 frame，master peak 为 0.989444，output overload 与 underflow 均为 0；完整 WAV 非静音且未发现 clipping。人工查看标题、Config、最快 Auto、Auto 正文、路线构图和结局返回标题六个 checkpoint，未见缺字、裁剪、拉伸、错层或残影。这关闭持久 Auto 首路线的 Headless E2，不替代正式完整音频听审、Skip 整路线、Config 剩余行为、第四条 clear route 后鉴赏或 Windows E3。

### Control 快进的活动消息语义

- Control 按键状态原本只让后续脚本 `wait` 在 command 执行时跳过；若按键在一条已显示消息上按下，旧 `Input` wait 仍会阻塞。VM 现在统一从 `skip_enabled`、`control_enabled`、Control pressed 和互斥 play mode 推导消息等待：有效快进把活动消息同 token 重绑定为 10 ms `Time`，释放 Control 后若尚未完成则恢复为 `Input`。movie/presentation/provider fence 不进入这条规则。
- Minori VM 与 provider 定向测试分别覆盖 gate、按下、释放和双向重绑定。真实八包 Headless 运行从标题前按住物理 Control，正文阶段不发送周期性 Enter，只在启动游戏、choice active 和返回标题 Exit 处提交必要确认。运行完成 25496 fixed steps、25498 个提交/栅格帧和 28 条物理输入，terminal、snapshot round-trip、自然解锁、既有完整路线 coverage hash 与零 diagnostic 均成立。
- 音频记录 20396544 frame，master peak 为 0.989372，output overload 与 underflow 均为 0；WAV 非静音且未 clipping。人工检查标题、路线和结局返回标题三个 checkpoint，未见缺字、裁剪、拉伸、错层或残影。这关闭按住 Control 的首路线 Headless E2；它不替代正式完整音频听审或 Windows E3。

### 持久 Skip 与 Config 重叠命中区

- 首次真实 Skip 运行在 `minori.play_mode=skip` observation 处阻断。输入与 Config 截图确认 Skip 单选没有变化；根因是左侧 slider 的 x 区间在 y 不匹配时提前返回 `None`，使同一 x 范围下方的 Auto/Skip 和一部分 Font 控件永远无法命中。修复后，slider y 范围仍优先解析，其余坐标继续进入原程序非 slider hit map；新增回归覆盖 Auto、Skip 和重叠边界的 Font previous。
- 第二次运行完全从原版 Config UI 选择 Skip 并 Apply，启动游戏后释放 Control，再用游戏菜单物理 pointer 启用持久 Skip。严格 play-mode observation 通过；正文阶段没有周期性 Enter，只在启动、choice active 和返回标题 Exit 处提交确认。运行完成 24901 fixed steps、24906 个提交/栅格帧和 52 条物理输入，terminal、snapshot round-trip、自然解锁、既有 coverage hash 与零 diagnostic 均成立。
- 音频记录 19920384 frame，master peak 为 0.989507，output overload 与 underflow 均为 0；WAV 非静音且未 clipping。人工检查标题、Config 默认、Skip 单选、Skip 启用、路线和返回标题六个 checkpoint；单选标记、日文文字、人物、背景和层次均未见阻断。这关闭持久 Skip 首路线 Headless E2。正式音频听审、Config 其余行为、鉴赏与 Windows E3 仍开放。

### backlog 多记录 retained text

- 合成 VM 回归补齐三条 message 的 cursor 边界：backlog 从最新记录开始，向上滚动依次进入更早记录并在首条钳制；内部正向移动同样在末条钳制。正文仍只存在于 local-private state 和一次性 lease。
- 首次真实多记录检查中，gauge ball 随 cursor 正确移动，但两个 idle 后 checkpoint 的正文为空。根因是 `system_ui_output` 在页面未变化时仍发送 `clear_text=true`，同时 retained scene 优化不会重发 lease。现在只有页面、cursor、输入或 restore 实际触发重画，以及 terminal 时才清除文字；未变化 tick 保留 Host retained text。单记录 provider 回归增加 idle tick 断言，要求不清除、不重复签发 lease。
- 修复后的真实八包短程使用物理滚轮打开 backlog，再连续查看两条更早记录并关闭。运行完成 771 fixed steps、776 个提交/栅格帧、55 条物理输入、snapshot round-trip 和零 diagnostic；这是未到 terminal 的定向 E2。人工查看打开、两次上翻和关闭四张画面，gauge 位置逐次变化，三条记录文字不同且完整，关闭后恢复当前消息。backlog 多记录翻页的当前契约至此关闭；正式音频听审和完整产品门禁不由该短程结果替代。

### Config 文字阴影

- 原程序 Config 的文字阴影状态现在直接控制既有 typed text presentation：启用时使用已经验证的 2 px 黑色 outline，关闭时提交 `outline=None`。字形 shaping、换行、裁剪和字体绑定仍由公共 CosmicText/Renderer2D 路径负责；Minori 没有新增私有文字渲染器、位图文字或失败 fallback。定向测试覆盖开关两态。
- 真实八包短程从标题进入 Config，以物理 pointer 关闭该选项并 Apply，再启动剧情直至首条真实消息。运行完成 384 fixed steps、388 个提交/栅格帧、34 条物理输入、snapshot round-trip 和零 diagnostic；该短程按计划未到 terminal。人工查看 Config 开关前后与首条消息，勾选状态正确变化，关闭阴影后的日文字形完整，未见裁剪、错层或残影。这只关闭文字阴影的定向 Headless E2，不代表完整 Config、正式音频 review 或 Windows E3 已完成。

## 2026-08-12

### message voice 资源绑定

- 对 89 个解密脚本与 `voice.paz` 做了脱敏全量绑定：18,319 条 message 中 7,049 条携带 voice，形成 7,047 个唯一 identity。7,015 个 identity 与 archive entry 直接同名；其余 32 个都带 `[volume,pan]` 修饰，去除修饰后分别唯一命中余下 32 个 entry。两侧基础 identity 集合完全闭合，没有缺失、歧义或路径非法项；公开记录只保留计数和集合 hash。
- IDA 复核 `CommandMessage` 后确认 voice 使用通用音频资源解析器：`[` 前是资源名，volume 默认 100 并限制到 0–100，pan 默认 0 并限制到 -100–100。每条新 message 先停止当前 voice stream，非空 voice 再单次播放。runtime 已据此通过公共 `LegacyAudioCommandV1` 发出 Ogg load/play/stop，没有 Luau、系统 codec 或命名 fallback。
- 原程序 backlog state 12 的 Enter 分支会取当前 `CLog` 记录、停止 voice，再调用同一播放函数；它不提交脚本 command。provider 因此只在 Backlog 页把物理 Enter 映射为当前记录重播，并保持原 message await 不变；与 wheel 同 tick 的冲突输入直接阻断。
- runtime state 硬切到 `astra.emu.minori.runtime_state.v21`，message/backlog 保存有界 voice URI、volume、pan 与 hash，restore 重新走 VFS/resource channel。107 个 `astra-emu-minori` library tests 和该 crate 全 targets clippy 通过。真实签名 plugin、Headless voice meter、backlog voice replay 与人工听审仍待重跑，不能把本次 E1 关闭写成新的 E2。
- 当前 v21 release CLI 与签名动态 plugin 已完成真实八包首路线复跑：33553 fixed steps、34172 个呈现帧、16957 条物理输入和 34 个 checkpoint，最终到达 terminal；snapshot round-trip、用户 save/restore、自然解锁、结局返回标题和零 diagnostic 均成立。backlog 页的物理 Enter 在不推进 VM 的情况下重播当前记录，随后仍恢复同一 message wait；Host 同 tick 返回旧输入 completion 的冲突已在 provider 边界显式消费，未通过丢弃结果或隐式推进规避。
- 模型检查 `backlog_open`、`backlog_voice_replay`、首个 choice 与返回标题画面，未发现重播导致的画面跳变、裁剪、错层或残留。完整 WAV 为 48 kHz 双声道、27311104 frame（约 568.981 秒），master peak 0.989529，离线量测无 full-scale sample，output overload 与 underflow 均为 0。该结果把 message voice 与 backlog replay 纳入同一条 Headless E2，但不替代具名人工整段听审、Windows E3 或第四条路线后的鉴赏验收。

### Auto/Skip 三态与原版菜单入口

- 再次 fetch 后确认 `origin/master` 仍是当前分支祖先，rebase 为 no-op，未触碰工作树中的任务改动。
- IDA 复核确认原程序只保存一个互斥 `playMode`：0 为 normal、1 为 auto、2 为 skip。`auto`/`skip` 动作再次触发会回到 normal；Control 是受 `enable_control` 与 `skip_enable` 双门控的临时快进，不等同于持久 skip。runtime 因此删除可重复推导的 `auto_mode`/`skip_mode`，改为 snapshot-safe 三态，并把 schema 硬切到 `astra.emu.minori.runtime_state.v20`。
- 原版游戏菜单首项根据配置偏好选择 Auto 或 Skip，点击时把对应模式写入同一个状态。参考舞台为 1280×720，菜单背景按右下角布局，首项图片命中区为 `[1125,1177) × [577,616)`。provider 只接受有界的 stage-space `pointer.x/y` 与主键物理输入；该点击即使同时完成 host 的 message await，也只切换模式，不把同一点击再次解释为普通正文推进。
- 原程序设置项 `messageSpeedAutoPlay` 缺省为 50，消息态以该值乘 10 ms 建立 Auto 文末等待。新 message 在 Auto 状态下因此发布 500 ms 的公共 time wait；当前已经显示的 message 不因中途切换而丢失。106 个 Minori 定向测试通过，新增覆盖三态互斥、snapshot、500 ms wait 与菜单点击冲突。
- release CLI 与动态 Minori library 已从当前源码重建，并使用只存在于本地进程环境的临时开发签名身份绑定最终 DLL；private key、manifest 和 artifact 均留在 ignored 私有目录。真实八包短程 Headless E2 推进 2255 fixed steps，严格 `minori.play_mode=auto` observation 通过，500 ms 后正文自然进入下一记录，两个 checkpoint 的 frame 与 observation hash 都发生变化，diagnostic 为空。人工查看两帧未见缺字、裁剪、拉伸或图层残留。
- 同一 build 又把 Auto 物理菜单输入与严格状态 observation 纳入完整首路线：在首条正文处切换 Auto、捕获 checkpoint，再以同一原版菜单动作恢复 Normal 后继续既有路线。运行完成 33498 fixed steps、16959 条输入和 34 个 checkpoint；snapshot round-trip、用户 save/restore、terminal、自然解锁与零 diagnostic 均成立，coverage hash 与未插入模式切换的 v20 完整路线一致，master output overload/underflow 为 0。人工查看 Auto checkpoint 未见新的视觉阻断。单次 500 ms 自动推进由前述短程 E2 单独证明；持久 Auto 跑完整条路线、Skip 菜单选择和 Windows E3 仍开放。

### 同一路线 choice、restore continuation 与结局媒体闭合

- fetch 后确认当前 `origin/master` 已是分支 `HEAD` 的祖先，因此本轮同步是 no-op；没有对包含任务改动的工作树做自动 stash 或重放。
- Manager 的 restore transaction 会先保存新开 session 的 rollback snapshot。启用全局进度、尚未发起首次 storage read 的状态是合法静止态，旧实现却以 `ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SAVE_PENDING` 阻断。现在 family snapshot 除 `astra.emu.minori.runtime_state.v20` 外，固定包含独立的 `astra.emu.minori.global_progress_snapshot.v1` section；它只保存 enabled/loaded 与已确认 clear flag hash，实际 pending I/O 仍禁止保存。restore 严格校验两个 section、schema 和 enabled identity，恢复后重新走 ordered platform storage，不把 key、正文、资源或路径放入 snapshot。
- restore 后的第一 tick 可以同时消费旧 message wait 并产生下一条 message。此前 retained scene 只在 wait 仍未完成时重发，导致新 Headless host 收到文字而当前 GPU scene 仍为 0×0。provider 现在在首个 restore output 没有 scene、但 stage identity 完整时，把 retained gameplay scene 与新文字放进同一 typed live transaction；已有 scene 时不重复提交，失败时不清除 pending restore 状态。该修复没有构造虚假 viewport、丢弃文字或 CPU fallback。
- 修复后的单次标题启动运行在同一签名 plugin、mount、profile 和物理输入序列中推进 33490 fixed steps、呈现 34108 帧、消费 16951 条输入并通过 33 个 checkpoint。首个 choice、实际分支的 post-choice、Config、backlog、用户 save/restore、真实开场和结局媒体、自然 unlock、结局返回标题与最终 Exit 都在同一个 run report 中；terminal、snapshot round-trip 与 user save/restore 均已通过，diagnostic 为空。
- 结局影片明确不可由 Control 跳过；runtime 等待 15000 个 fixed tick 使 media fence 自然完成后才继续返回标题。音频 pre-master peak 为 2.711029，master output peak 为 0.989551，master overload 与 underflow 都为 0。
- 模型逐项查看了正式 bundle 的 33 个 required checkpoint。标题、Config、影片、普通剧情、多人构图、choice、post-choice、转场、结局和返回标题都未发现缺字、裁剪、拉伸、错层、透明残留或影片比例阻断。白场中的重复小标记只按当前脚本演出记录，未取得原版同点画面前不声明逐像素一致。
- `prepare-review` 的自动门禁为通过；local-private review 对 33 个 frame 都给出通过，但完整 WAV 尚未逐段听审，因此 `full_audio` 保留 `ASTRA_HEADLESS_REVIEW_AUDIO_LISTEN_PENDING`，`validate-review` 按设计返回 `ASTRA_HEADLESS_REVIEW_BLOCKED`。这次运行补齐同一路线 choice 与结局媒体，不替代此前两次重复运行的确定性证据，也不关闭正式音频 review、第四条 clear route 后的 `Memories`、完整系统页或 Windows E3。
- 复核 validator 时发现，bundle 虽强制选择完整 WAV，旧 `ReviewRecord` 却没有绑定音频 verdict；省略音频审查仍可能通过正式 preflight。公共 Headless review 已硬切 v3：记录 run report hash、review bundle hash，并用 typed artifact verdict 绑定每个 selected audio 的 role/path/hash。视觉和音频 verdict 各自保存 reviewer provenance，完整 WAV 只有具名人工 verdict 才能通过。CLI validator 会重新计算实际 artifact hash；缺失、额外、失败、hash 漂移、错误 reviewer kind 或 bundle 漂移都会阻断，不能用 frame verdict 冒充音频审查。protocol、CLI validator、Python platform acceptance 和 release preflight 的定向回归均已通过。
- 为后续具名听审，完整 27260416 frame、48 kHz、双声道 WAV 已在 ignored 私有目录按连续 60 秒区间切成 10 段。catalog 保存源文件与分段 hash、起始 frame 和 frame count；覆盖总数与源 frame count 完全一致，源 hash 匹配 review bundle。该准备动作不等于听审通过，分段文件和 catalog 不进入 Git、report 或 package。

## 2026-08-11

### 平台全局进度与完整单路线 Headless E2

- Family runtime 的 ordered provider wait 现在携带有界的 `request_id/provider_id/operation/key/payload`，provider result 返回同样受 1 MiB 上限约束的 payload。Family ABI 与 Provider ABI 两层 wire DTO 都做显式映射，不传平台对象、路径或 handle。Headless composition 只绑定 `astra.platform.storage`，用现有 `PlatformHostClient` 的 `begin/write/commit` 和 `read` 完成原子存储；未知 provider、operation、slot 或不匹配的结果直接阻断。
- Minori 使用 `astra.emu.minori.global_progress.v1` 保存四个已由原程序确认的 clear flag hash。首次 fixed tick 先读平台 slot，缺失表示空状态；路线写入已确认 flag 后，provider 必须等平台原子提交完成才允许 terminal。restore 保留同一 provider session 已确认的全局进度，并把它重新并入旧 snapshot，不能通过恢复旧进度回滚解锁。key、正文、资源和本地路径不进入 report。
- 两次同构的真实 Headless 运行都从标题进入 Config，再开始完整路线；途中用物理滚轮打开并关闭 backlog，以 F5/F9 完成用户 save/restore，并播放真实 AVI/WMV3/PCM 影片。每次推进 31006 fixed steps、呈现 2662 帧并消费全部输入；第二次为 20060 条输入，比第一次多一个只比较 hash 的自然解锁断言。两次均到达 terminal，snapshot round-trip 与用户 save/restore 成立，diagnostic 为 0。
- 两次运行的 visual trace、runtime state trace、route terminal、coverage 和 audio meter hash 完全一致。pre-master peak 为 2.711029，说明素材混合前确有超限；共享 Kira master limiter 后的 output peak 为 0.989551，output overload 与 underflow 都为 0。这个结论只覆盖最终输出安全，不把 limiter 写成原版混音 parity。
- 模型实际检查了第二次运行的全部 30 个 checkpoint。标题、Config、影片、backlog、多人构图、近景 CG、白场/黑场转场和路线前中后画面均有真实内容；日文字形完整，未发现横向裁剪、非预期拉伸、透明边缘错误、人物残留或影片比例错误。白场与黑场是脚本演出的一部分，正文轮廓保持可读。模型结论是本次 Headless 质量审查通过，不等价于原版逐像素一致。
- 第二次运行在 terminal 后严格等待 `blackboard.minori.gallery_unlock_count == hash("1")`，证明首条路线自然产生一个脱敏解锁项；平台私有 slot 也由同一 ordered write 成功提交。原版标题资源表明 `Memories` 菜单只在第四条已确认 clear route 后出现，首条路线完成后仍使用不含鉴赏入口的标题 variant。因此本次不能伪造 CG/BGM/回想 checkpoint；这些页面与结局返回标题继续保留为后续多路线系统 UI 工作，Windows E3 也仍开放。
- 增量全库测试随后发现，直接从 gameplay 打开 skippable movie 时，`Play` 曾把 instruction count 当作 live effect sequence，与首 tick blackboard observation 撞号。movie command 现统一从 VM effect allocator 取 sequence；100 个 Minori 单元测试通过。修复后的真实路线又连续运行两次，两次上述计数、visual/runtime/terminal/coverage/audio hash 和自然解锁断言全部一致，因此本节数字只引用修复后的运行。

## 2026-08-09

### ABI v8 rebase、纯 Rust AVI 与首条路线 terminal

- 分支已 rebase 到当时本机 `master` 的 `adf25787`。由于网络更新未确认，本文只记录本地基线。恢复 Minori runtime 后，provider 已改用 Family ABI v8 与 Provider ABI v4 的 typed scene/audio/video/text/control DTO；旧 effect envelope、postcard live payload 和兼容 fallback 没有恢复。
- 重新核对 `mov` role 后，5 个授权条目均为 RIFF/AVI，不是先前记录的 Matroska/MPEG wrapper。视频为 WMV3 1280×720、24 fps，音频为 PCM 48 kHz 双声道 16-bit。range-backed AVI reader 每次最多读取 4 MiB，WMV3 与 PCM 都走仓库现有纯 Rust组件；5 个条目共解出 17480 个视频 sample，零长度 sample 只作为 dropped frame 计数。
- Headless rebase 后改走 retained GPU scene，旧文本适配仍依赖 CPU underlay，真实运行因而阻断。现有 CosmicText `TextRenderResourceOwner` 直接产出 glyph lifecycle 和 draw command，并与同一 `SceneFrame` 合并；没有整帧回读，也没有 CPU 或字体 fallback。多个消息在采样帧之间合并时，glyph mutation 会按平台 resident state 归并，避免同一 resource id 在单帧内反复 release/upload。
- 背景或立绘复用 texture id 但尺寸变化时，Host 现在用 typed `reset_resources` 原子重建当前帧声明的全部纹理。source revision 与 GPU generation 不再被当成同一个概念，也不通过忽略尺寸变化继续运行。
- 最终私有输入序列全程按住 Control，只在 `runtime.awaiting_input` 出现时发送 Enter。运行消费 8033 条消息，推进 34567 fixed ticks，提交 5761 帧，自然到达稳定 route terminal；snapshot round-trip、10 个 checkpoint、3431 个脱敏 VM coverage id、VFS/resource identity、音频 meter 和完整 shutdown 均通过，diagnostic 为 0。报告只保留计数与 hash，不含正文、key、资源内容或本地路径。
- 首轮模型检查发现最后一个 checkpoint 的白场正文对比度不足。补齐原程序文字轮廓后，以同一输入、mount 和签名 plugin 重跑仍消费 8033 条消息、推进 34567 fixed ticks、提交 5761 帧并自然到达 terminal；snapshot round-trip、10 个 checkpoint、3431 个脱敏 VM coverage id 和零 diagnostic 均保持不变。模型复核全部 10 个新 checkpoint，日文字形、人物比例、背景尺寸、图层和透明边缘正常，末尾白场正文清晰可读，没有缺字方框、横向裁剪、非预期拉伸或旧帧残留。这项剧情路线视觉 blocker 已关闭，但不外推为原版像素一致。
- route terminal、非空 VM coverage、标题启动、Config、用户 save/restore、backlog 和真实影片 checkpoint 已有 Headless E2 证据。trace 收集由 host 显式传入的 `astra.hosted_trace_profile=evidence` 开启，shipping profile 不收集，未知 profile 直接阻断。global progress、CG/BGM/回想鉴赏和 Windows E3 仍未覆盖，不把这次剧情路线写成完整模拟器完成。
- 针对白场低对比度问题，原程序反编译确认 `CTextDrawer` 构造时默认开启轮廓：glyph mask 先以黑色 alpha 层扩展 2 像素，再绘制正文颜色。AstraEMU 因此在通用 typed text presentation 中加入有界 outline，而不是按 Minori layout id 做 host 特判；Host 复用同一 CosmicText glyph resource，以 Renderer2D glyph run 组合轮廓和正文。合成白底回归和同路线真实 checkpoint 复验均已通过。
- 继续核对原程序的选择界面后确认：单次选择严格截断为最多 4 项，每项分别构建 blur、focus、active 三态。授权样本内的首个资源名实际为大小写敏感的 `SelectBLur.png`，另两个为 `SelectFocus.png` 和 `SelectActive.png`；文本字号为 26 px，各项按资源高度纵向排列，整组按 stage 宽高居中，交互 id 为 `SELECT%d`。runtime 现已按 archive identity 加载并校验三态资源，按当前焦点选择 blur/focus 纹理，并把一至四项正文作为单批 ephemeral text lease 交给 CosmicText；选项正文水平居中，确认后通过 ABI 的 `clear_text` 信号释放 retained glyph。parser/VM/provider/host 的增量测试已经通过，但真实路线尚未生成选择 checkpoint，active 状态的指针按压语义也未形成视觉证据。

### 纠正旧结论

本页 2026-08-03 条目保留为历史过程。其中 `af564c07` rebase、481-tick slice、Matroska/MPEG movie census、Firefly/select/stand 尚未实现等描述，均已由本节的新代码和证据覆盖；引用当前状态时应以 2026-08-09 为准。

## 2026-08-03

### rebase 后的 runtime 与 Headless E2 复核

- 当前实现已 rebase 到 `master` 的 `af564c07`；Minori 分支保留在其上继续开发。动态 `cdylib` 的 abi-stable root symbol 现在会在启用 `dynamic-plugin-export` 时保留，native package signer 可以读取 descriptor 并完成签名校验。
- family provider 不再把宿主传入的 canonical input edge 一律判为未验证。对 `enter`、`space` 和 `pointer.primary` 的按下 edge，若当前是 input wait，则解析为同一个 wait；已完成的 await 与重复按下同时出现会阻断。无匹配等待的物理 edge 只作为已验证的输入通道数据消费，不猜测成脚本 action。provider result 仍要求匹配显式 Provider wait。
- 使用私有签名动态 plugin、真实挂载 profile 和 `test.sc` 链接入口完成一次 Headless slice：481 fixed steps、24 个呈现帧、27 条输入消息、snapshot round-trip 成立、diagnostic 数为 0；artifact 中有非静音音频且无 clipping。该 slice 未到 terminal，不能作为完整路线或 E3 证据。
- 模型视觉检查了全部五个 checkpoint。启动帧的竖排日文标题字形完整、比例正确；中段三个 checkpoint 与启动帧保持同一标题画面，说明当前输入时间点没有形成可见消息层；末帧进入非空的暗色场景。自动 report 的 `passed` 只表示 Headless 生命周期、资源读取、快照和 artifact 门禁通过，不覆盖消息内容视觉正确性。

### movie 全量 census（覆盖此前的五个 MPEG 结论）

- 真实 `mov` role 的 full scan 现在得到 5 个 entry、882835050 decoded bytes；聚合容器分类为 `matroska_wrapped: 1`、`mpeg_pes_wrapped: 2`、`unrecognized: 2`。报告只保存计数和 hash，不保存 entry 名称、字节或本地路径。
- `MpegRangeDecoder` 的纯 Rust 路径已补齐跨 chunk start-code telemetry 和 EOF trailing PES flush；这只证明 MPEG-PS/PES 路径的读取边界，不等价于 Matroska 或两个未识别容器的 codec 支持。缺少明确 container/codec provider 时继续返回稳定 blocker，不调用平台 codec 或 fallback。

### 当前 blocker

- 该阶段以原版入口继续运行时，曾在 `Firefly` 和 secondary effect slot 阻断；两者后来按样本与 IDA 合同补齐了局部实现。其他未知 effect 仍不按名称猜测，继续返回 `ASTRA_EMU_MINORI_RUNTIME_EFFECT_KIND`。movie 的 Matroska/未识别容器同样未关闭。
- 因此当前证据边界是“VFS + 签名 plugin + 可复现 Headless slice + 受限视觉检查”，不是完整 VM、完整路线、电影播放、自然解锁或 Windows E3。

### Control 快进语义（IDA 复核）

- 使用同一签名动态 plugin 的私有 Headless control sequence 已完成 300 fixed steps、54 个实际呈现帧、9 条物理输入、snapshot round-trip 和非静音音频，diagnostic 为 0；Control 让时间等待路径继续推进，但仍未到达 terminal，不能视为完整路线或 E3。

- 原程序的 `VK_CONTROL` 快进路径不是全局输入动作。IDA 复核确认它同时受两组独立状态门控：`enable_control`/`disable_control` 控制是否接收 Control，`skip_enable`/`skip_disable` 控制脚本是否允许跳过；原程序初始化时后者为 enabled。runtime state v20 分别保存 `control_enabled`、`skip_enabled`、物理按键状态与 play mode；有效快进由这些权威状态即时推导。任一 disable pragma 都立即关闭对应快进路径，但不伪造按键 release；恢复后按同一组合重建状态。
- 生效时只跳过已确认的 `.wait` 时间命令，并继续在同一个固定 tick 内执行后续已验证命令；Host 仍逐 tick 调用 provider，不直接跳过 deterministic tick，也不自动完成 message input、media fence、presentation fence 或 provider wait。
- 未知 pragma 仍返回 `ASTRA_EMU_MINORI_RUNTIME_PRAGMA` blocking diagnostic。runtime snapshot schema 已升为 `astra.emu.minori.runtime_state.v20`，保存两组 gate、Control pressed 与互斥 play mode，不保存可重复推导的 effective skip 布尔值。
- focused runtime/provider tests 已覆盖 pragma gate、未知 pragma、Control edge 和 timer fast-forward。该语义尚未证明完整原版演出、影片或首条攻略路线，因此不能扩大为全局 skip 或 E3 parity 结论。

### runtime state v19 单路线复验（2026 年 8 月 12 日，已由 v20 schema 取代）

- 分支同步检查确认最新 `master` 已是当前提交的祖先，因此 rebase 没有产生新提交。随后使用普通 Cargo target 重建 release CLI、签名动态 Minori plugin 和 manifest；旧 v193 plugin、manifest 与报告没有被当作当前构建证据。
- 第一次复验错误地显式指定了脚本 entry，按既有合同直接进入剧情并跳过标题页。旧输入序列在配置页 observation 超时，Headless 以 `ASTRA_EMU_HEADLESS_AWAIT_TIMEOUT_INPUT_PREDICATE` 阻断。去掉该参数后，同一 v19 二进制按默认标题入口重跑，未修改输入序列或放宽 timeout。
- 当前构建完成同一条首路线：33490 fixed steps、34108 个呈现帧、16951 条物理输入、33 个 required checkpoint，最终到达 route terminal。snapshot round-trip、用户 save/restore、影片 fence、首个选择、自然 unlock、返回标题和 Exit 均通过，diagnostic 为空。
- 全资源审计覆盖 14502 个 VFS entry、43818 个 range 和 6624958365 byte；最大 range 为 4 MiB。完整音频为 48 kHz 双声道、27260416 frame，master peak 为 0.989551，overload 与 underflow 均为 0。34 个 manifest artifact 已逐项复算 hash，没有 identity drift。
- v19 的 33 张 checkpoint PNG 与 v193 逐项 hash 相同。模型重新查看了标题、配置、影片、首个选择、结局和返回标题，未见缺字、拉伸、图层错位或残留。其余 27 张沿用相同 hash 的既有全量视觉审查结论。完整 WAV 仍缺具名人工逐段听审，因此正式 review 继续由 `ASTRA_HEADLESS_REVIEW_AUDIO_LISTEN_PENDING` 阻断；这次结果是当前构建的 Headless E2，不是 Windows E3 或完整产品验收。

## 2026-08-02

### PAZ 影片 reader 与 cache 边界复核

后续的真实八包 cache-enabled full verify 已完成（8 个 source、14,502 个 entry、43,818 次 range read、`cache_hit_count=43,594`）；本节早期“仍需独立真实 cache hit”只保留为当时的历史状态，跨运行 identity、淘汰和损坏恢复仍未由该轮关闭。

- 对照 GARbro 的 `MovPazArchive`，确认 v0 影片索引表是 plaintext-to-encrypted 映射；读取时必须构造逆表。新增非对合 substitution fixture，避免把恰好可逆的测试数据误当作格式证据。当前授权样本为 v2，此项不构成对其媒体内容的解码验收。
- `MINORI_READER_ID` 升级为 `astra.emu.minori.paz.v2`，并进入 plaintext cache 的 codec identity。旧 reader 写出的明文 cache 因而不会跨实现版本复用；新版本仍需由独立的真实 cache hit 轮次验证。
- v1/v2 的 RC4 entry key 改由 index 中保留的原始 CP932 名称字节派生，仅对 ASCII 字节做格式要求的大小写归一化。这样避免 Unicode decode/re-encode 改变密钥；原始字节只驻留 mount session 的 opaque descriptor，不进入 save、report 或 cache identity。
- 真实首路线已到达影片指令。公共 VFS 已提供 range-backed reader，Minori 对未压缩 movie entry 以 entry-relative transform 直接读取请求范围；它不再因首个 header probe 物化整个影片。压缩 entry 仍走完整、受上限的解压路径，不能冒充流式实现。
- 原程序的影片启动路径按扩展名选择文件流媒体图。AstraEMU 不复用该系统路径：Minori host 只用有界 reader 与显式 AstraMedia FFmpeg incremental provider。私有全量有界扫描确认五项授权 movie 都在 wrapper 后包含 MPEG start code；此前固定 4 MiB probe 把四项误归为未知容器，不能再作为 codec 结论。runtime 交给共享 FFmpeg demux/codec 逐 packet 读取，不物化第二份 caller-owned 媒体。这不是 E2 路线通过证据。

### MPEG 流式绑定

- 已将已识别 MPEG entry 交给 `MpegRangeDecoder`：reader 只保留 64 KiB 输入块和最多 256 个待消费事件，帧与音频不物化为整片媒体或历史帧队列。runtime 以 4 MiB 上限 VFS range chunk 扫描 wrapper，并保留三个字节处理跨块 start code；MPEG 前缀偏移在创建 decoder 时固定，VFS range transform 仍由 Minori mount session 执行。
- 真实 Headless 重试暴露了通用 raw-range 路径错误地把非 movie entry 的任意子范围送入 Blowfish；这与 block cipher 的 8-byte 对齐契约冲突。现仅 movie 的 entry-relative RC4 transform 可走随机范围读取；其余 entry 强制回到完整 aligned decrypt 与 truncate 路径。GARbro `OpenEntry` 合同进一步确认 zlib 只由 index 的 `IsPacked` 决定，不可按 archive role 推断。合成 v0/v1/v2 fixture 现分别覆盖普通 unpacked random read、packed zlib read 和 movie RC4 range。
- 视频 frame 的 PTS、尺寸、RGBA 长度与 sequence 都在进入 Renderer2D 前校验。音频 chunk 只接受有限的 mono/stereo、有限 sample、交错 frame 对齐与单调 PTS；pending event 和 mixer queue 都有独立上限。错误格式、时间倒退、队列超限或 identity 冲突均为 `ASTRA_EMU_MINORI_MPEG_*` blocking diagnostic。
- MPEG 音频改为流式提交现有 Headless audio executor。它不走整片 decode 或私有 mixer；首次 chunk 必须精确匹配 host 显式 audio output format，之后格式漂移直接阻断。视频 EOF 只标记 audio EOS，media fence 会等待已提交音频 drain 后再完成。
- 真实重跑说明 MPEG stream 在首帧前并不构成视觉变化；旧 Host 却会立即调用 `present`，并因没有前序 scene frame 错误返回 `ASTRA_EMU_HEADLESS_BASE_FRAME_MISSING`。现仅在实际视频 frame 更替时请求呈现；若首个可见 frame 没有 underlay，则由已验证的 movie stage 尺寸创建 renderer clear canvas 后合成该 frame。它不是资源、解码器或系统 fallback；空流到 EOF 仍以 `ASTRA_EMU_MINORI_MPEG_VIDEO_FRAME_MISSING` 阻断。
- 该轮仅有 range decoder、格式边界、跨块 wrapper 扫描和 Headless binding 的定向 Rust 回归。尚未以授权样本完成 MPEG 影片的实际 Headless E2；扫描签名不是完整 container/codec 解码成功证据。

## 2026-08-01

### 安装包 E2 回归：同 tick underlay 与输入 wire format

- 为所有 family 通用的 desktop packager 增加显式 `--family` 选择；Minori 包包含该 family 的已签名动态库、manifest 和独立第三方 notice。FVP 不再是路径或文件名假设。
- 首次从安装包启动真实样本时，旧私有输入文件因 externally-tagged event、PascalCase button state 和字面量 `\\n` 被严格拒绝。按当前 `astra.user_input_sequence.v1` 重新序列化后才继续；reader 没有接受旧 wire format 的兼容分支。
- 发现并修复 Headless 的有序合成缺口：`render_resource_frame` 与文本 capture 位于同一 runtime step 时，Host 过去在 effect 循环结束后才把资源帧栅格化，且没有写入 `underlay_frame`，导致 `ASTRA_EMU_HEADLESS_TEXT_UNDERLAY_MISSING`。现在只会从该 step 已提交的显式 render frame 生成 underlay；没有 render frame 仍保持 blocking，不生成替代画面。
- 修复后，安装包对授权样本完成一轮 local-private Headless E2：400 fixed ticks、12 个提交帧、1 个 checkpoint、snapshot 校验通过、无 runtime diagnostic。该轮只观测入口初始演出，未发送未验证的对话推进或选项输入，因而不是 terminal route、完整视觉审查或 Windows E3。

### 输入等待观测与后续语句边界

- Headless runner 现在为活动的输入等待写入 `runtime.awaiting_input` 观测值。它只哈希已排序的物理输入 mask，不携带 await token、脚本位置、正文或资源名；输入序列可以据此在固定 tick 上发出同 tick 的 press/release，而不依赖未受约束的时间猜测。
- 该观测已让私有输入回归越过首个消息等待，并命中单 operand、`mode=0`。随后重新打开原程序的 IDA session：`CMessagePanel` 的 mode switch 中，case 0 不加载 panel asset，而 case 1 才加载 `msgPanel.png`。runtime 因此把 mode 0 表示为清除当前可见 panel，并重发无 panel 的资源帧；没有把它猜作默认 panel 或过渡操作。mode 2–10、第二个过渡参数和文件名覆盖仍保持 blocking。

### Effect operand contract 修正

- 新的安装包 E2 在通过 `.panel 0` 后命中 `.effect CrossFade2` 单 operand 形式。原程序 `CommandEffect` parser 证实第一个 operand 是 effect id，第二个 operand 才是可选的资源规格；后续三个整数缺省为 `-1`。此前把首个 operand 当作资源序列、把两个整数当作已确认时钟参数的实现与文档均已撤回。
- 对该已确认的零资源 `CrossFade2`，原对象替换 primary effect slot，但没有解析出资源帧。runtime 以清除活动 effect、递增确定性 sequence 和不提交替代 presentation 表达同一状态，不生成自交叉淡入、静态替代帧或资源猜测。带资源或数值配置的 effect 仍是 blocking 边界，待对象 tick、资源解析和 composite contract 一并验证。
- 同一真实 Headless E2 随后命中四 operand 的 `CrossFade2`。原程序 parser 与对象初始化交叉确认其形态为 effect id、冒号分隔资源规格和两个直接保存的整数；只有至少两个解析资源才打开双帧路径。两个整数在对象 update/composite 中的含义尚未闭合，runtime 继续阻断，不沿用合成 fixture 的命名或步进推测。
- 该路径的资源规格为单独的 `*`。反编译确认 effect 对象仍先进行通用资源查询，查询未产生资源对象时不会打开双帧分支；它仅替换 primary effect slot。runtime 因此将此 sentinel 与无资源的单 operand 形式等价处理：清除已有 effect presentation、递增确定性 sequence，不把 `*` 映射为背景文件或伪造帧。其后的数值字段在没有资源对象时不会参与可见更新。

### 原程序选择命令复核

通过原版 `ScriptPool` command registry、`CommandSelect` vtable、parser 与选择 UI 创建路径交叉确认：`.select` 最多接受四个 positional option；每个 option 按第一个 `:` 切为显示文本和目标字段。UI 也只接受一至四项。

当前不能从这些事实推出右侧字段必然是 label，也不能确认选择后写入变量、直接跳转还是经额外 script state 间接分派。runtime 因此保持 fail-closed，不把 `select` 变成猜测性的跳转。后续需要从选择确认回调追到脚本 runner，再用本地样本的脱敏 control-flow evidence 验证。

同轮加入 `astra.emu.route_coverage.v1`。Minori runtime 仅发出由 script identity、command ordinal 和 opcode 派生的 coverage id；Host/E3 只保存 namespace、聚合 hash 与计数，不保存 URI、正文、operand 或资源内容。Windows 原生 E3 预检可以校验这一信号，但完整路线尚未产生 terminal 和 coverage 的授权基线，不能据此声明 E3。

`astra-emu-cli run/headless` 的准备阶段不再为 FVP 保留 desktop reader 分支。两个当前 family 均经显式注册的 `LegacyVfsFamilyFactory` 挂载，并统一以 `LegacyMountedVfsReaderAdapter` 向 runtime plugin 提供资源、范围读取审计和 full verify。启动 entry 必须由调用方给出已挂载的完整 URI；缺失或不属于 manifest 的 URI 直接阻断，不能按扩展名或扫描顺序猜测脚本。

## 2026-07-21

### 目标

在 Family VFS 公共化基础上建立 Minori runtime 的可信输入面：完整识别真实 PAZ 集合，固定 GARbro 格式 contract，并让 ANI/SQZ 输出进入 AstraEngine 已有图像管线。VM、系统 UI 与 Headless 路线验收尚未到可声明完成的阶段。

### 本次完成

- 分支已 rebase 到当前 `master`，保留 `family-core`、`family-support`、通用 VFS CLI 和 Minori 私有 importer。
- 重新递归扫描授权样本，纠正“只有六包”的旧结论。实际为 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive；`bg` 包含主包和 A–J 十个连续分卷，全目录共 18 个物理文件。
- 将这次检查固化为 `astra-emu-minori-cli scan-archives`。扫描器递归检查分卷连续性、重复项、空文件和 symlink；当前样本的 18 个文件合计 5742470010 bytes，inventory hash 为 `sha256:5a5729b8fcaec7cf218fa211e0b76e89af162f3666fd8b3c794e550049e16637`。
- 纯 Rust GARbro importer 现在要求八个 role，并生成与八包集合一致的 private profile。真实挂载解开八个 index，八包 14502-entry decoded full verify 已通过；不再把六包 9837-entry 结果写成全包证据。
- `LegacyMountedVfs::read_dir` 允许合法的 mount root 和单个结尾 `/`，file URI 的严格规则不变。新增 core 与 Minori fixture 回归测试。
- 依据固定 GARbro revision 实现 ANI 与 SQZ 的纯 Rust 有界 adapter。ANI 覆盖 BGRA32、BGR24、BGR565 和 Gray8；SQZ 对 zlib BGRA32 frame 做精确输出长度校验。两者都输出 `image::RgbaImage`，没有 Minori 私有渲染器。
- 新增 `census-media`，并对真实 `bg`/`bgm` 完成逐 entry、逐 ANI/SQZ frame 验证：2655 PNG、1951 ANI/6723 frames、9 SQZ/224 frames、49 Ogg 和 1 个 metadata database。
- 建立可序列化的 Minori runtime state，覆盖 PC、local/global 变量、wait、message/choice 引用、图层、音频、影片、系统页、鉴赏解锁和 deterministic counter。当前执行 `set/setglobal/label/goto/if/wait/chain/end`；未确认命令返回 `ASTRA_EMU_MINORI_RUNTIME_OPCODE`，不会静默跳过。
- 通过 IDA MCP 对原版入口的 command registry、RTTI、vtable 和 handler 调用点做了交叉验证。结果否定了此前讨论的普通 call 假设：`chain` 先执行与 `end` 相同的结束流程，再把参数写入全局 `NEXT`，没有 call stack 或 return path。runtime 已改成尾链式 VFS 切换；snapshot schema 升到 v2，恢复时重新读取 active script 并校验 hash。
- 同一轮反编译确认了 assignment 的 3/5-token 结构、local/global store 查询顺序、`if` 的六种比较运算，以及 `wait` 的 10 ms timer tick。旧 VM 把 `.wait` 参数直接当 milliseconds、把 `.set x += 1` 当原生语法，均与 handler 不符，现已按根因修正。
- snapshot 使用 postcard round-trip，并由测试验证 wait continuation 与 state hash。实现过程中发现 internally tagged enum 无法稳定通过 postcard，已改为二进制格式兼容的 enum 表达，没有保留只可写不可读的 snapshot。
- `astra-emu-minori` 已产出 `rlib`/`cdylib`，实现完整 `LegacyRuntimeProvider` ABI surface 与 host VFS FFI。公共 `LegacyMountedVfsReaderAdapter` 把已解密 mount 绑定到 runtime reader，revision 由 reader、profile、source、entry 和 method identity 派生，不暴露 source path。
- provider lifecycle fixture 已覆盖 open、连续 wait tick、await completion、save、restore 和 shutdown。真实 Headless 长等待暴露了 AwaitToken 重复提交：family 在每个等待 tick 重发同一 token，而 Manager 正确阻断重复 token。运行时现只在创建等待时提交一次 request，后续 tick 只保留 family 状态；回归测试覆盖这一边界，没有放宽公共 Await 校验。
- 原程序 tokenizer 已确认按每个 ASCII space/tab 切分 operand，连续分隔符会保留为空的 positional operand；逗号和引号不具备分隔或引用语义。真实脚本的重复空格只出现在 `message`：双空格 3883 条、三空格 7387 条。parser、typed operand 和 census 现已使用同一规则，避免丢失空 voice/speaker 字段，也不在 parser 层重写商业文本或资源规格。
- `CommandMessage` 的 parse/execute contract 已闭合并进入 VM。确定性 state 只保存 message id 与三个字段 hash；正文和 speaker 通过一次性 `TextCapture` lease 交给 host，消息随后等待显式物理输入。短参数行按原程序构造器默认值执行空更新，不作静默跳过。
- `transition` 与 `stage` 的字段顺序已经由 vtable、parse handler、stage core 调用和入口样本交叉确认。stage 依次接收前景、可选前景坐标、背景、背景坐标和最多十组 stand pair；`*` 表示空层。背景/前景绑定 `bg`，stand 绑定 `st`。stand position 仍是未解释的引擎参数，不能当像素坐标。
- 第二层音频规格已经还原为 `resource[volume,pan]`，包括 C `%d` 数值前缀、默认值、范围夹取和缺右括号行为。BGM/SE 的 fade-in、fade-out 与 SE repeat flag 也由调用路径确认。
- census v3 对 811 条音频引用执行脱敏绑定：401 条非 `*` 引用全部精确、唯一命中 `bgm` 或 `se` entry；410 条未命中项全部为 `*`，没有普通资源缺失或大小写歧义。IDA 已确认 `*` 停止 BGM、对应 SE bus 或 voice stream，并使用各命令的 fade-out 参数。
- 非控制型 `playBGM/playSE/playSE2/playSE3` 已通过稳定 URI 发出公共 audio effect。provider 在发 effect 前检查 VFS stat、非空和 1 GiB 上限；host 仍经 session resource channel 读取内容。snapshot schema 升为 v3，保存 audio pan 与 bus state。
- `astra-emu-cli run/headless` 已硬切到 `--family` 与 `--mount-profile`，Minori 通过显式静态 factory registry 挂载八包，再加载签名动态 `cdylib`。旧 `--engine` 被 CLI parser 拒绝，未保留 alias 或 fallback。
- Headless surface 在 runtime 尚未提交 scene 时可以正常销毁，不再让 `surface.capture` 清理错误掩盖 family 根因；没有为失败路径生成空白替代帧。
- `stage` 只提交 `astra.emu.render_resource_frame.v1`：effect 保存 VFS URI、编码 hash、已验证尺寸和绘制指令，不保存商业像素。Headless 与 Manager 通过 session resource channel 取回编码数据，再交给唯一显式绑定的 Astra `DecodeProviderRegistry`；纯 Rust `ImageDecodeProvider` 是 packaged-eligible 主 provider，不走 fallback。Host 校验编码 hash、RGBA hash 和尺寸后才生成临时 `LegacyRenderFrameV1`。迁移后的真实八包运行前 5 个 fixed tick 已通过：入口 tail-chain、BGM、SE、全黑背景和竖排标题共提交 2 帧，两个 checkpoint 均为 1280×720，视觉发生变化，snapshot round-trip 与音频 artifact 同时成立。新旧路径的 checkpoint hash 与 visual trace hash 完全一致；截图与正文只留在 ignored 私有 artifact。
- 当时的 effect 实现依据合成 fixture 将首个 operand 后的字段解释为资源序列与时钟参数。该解释已被后续真实单 operand E2 与原程序 parser 推翻，现仅作为已撤回的研究记录保留；生产 runtime 不再使用该路径。
- 资源规格的分隔、单资源对象行为、时钟参数和 frame composite 仍需以原程序对象生命周期与真实样本逐项闭合；当前不再把零资源或单资源情形解释为静态替代 presentation。
- 命令注册与构造路径确认 `.effect`、`.effect2` 复用同一 `CommandEffect`，但由构造参数绑定到两个独立的原程序 effect slot。runtime 已为第二 slot 增加独立 state、timeline、composition 与 snapshot，未把 `.effect2` 合并到首层。
- 当前样本 census 只观察到 `SnowH` 和 `fadeout`。IDA 跟踪到 `SnowH` 构造函数绑定 `snowS.png`、`snowM.png`、`snowL.png`，创建 50 个粒子；更新函数使用定点坐标、横向速度、上下抖动方向和 16 ms alpha 累计。实现仅开放这两种已确认形式，其他 kind 保持 blocking。
- `astra.emu.minori.runtime_state` 因新增第二 slot 状态从 v13 硬切到 v14。87 个 `astra-emu-minori` library tests 全部通过，其中新增测试覆盖第一/第二 slot 隔离、同 seed 确定性、定点移动、fadeout、损坏 snapshot 阻断以及三纹理/50 draw 的 provider 合成。这里是 E1 证据；真实签名 plugin 与 Headless 路线仍需重跑。
- `.panel` parser、`CMessagePanel` 调用和 mode switch 已确认：最多接收两个整数和一个字符串，缺省值分别为 `0`、`-1` 和空串；mode 0 清除当前可见 panel，mode 1 选择 `msgPanel.png`，mode 与文件名分别以 `!panel_Mode`、`!panel_Filename` 进入存档。runtime 现实现无附加 operand 的 `.panel 0` 与 `.panel 1`；其他 mode、过渡参数和资源覆盖继续阻断。snapshot schema 随可见 effect frame 与 panel state升为 v6。
- IDA 进一步确认 mode 1 的坐标计算：横坐标取 panel 全局 x，纵坐标为 viewport 高度减图片高度再加 64。真实资源为 263 px 高，因此 720p viewport 中从 y=521 开始绘制，底部 64 px 按原程序语义落在 viewport 外。首次视觉检查发现实现错误地把 panel 放在顶部；现已修正根因并用尺寸回归测试固定，不以视觉容差掩盖。
- 修正 positional tokenizer 与 input-await edge routing 后，真实八包 Headless 运行到 373 个 fixed tick：实际提交 9 帧，保存黑场、标题、可见 CrossFade2、panel 和前两条 message 六个 checkpoint，消费 16 条物理输入，snapshot round-trip 为 true，diagnostic 为 0。运行读取 10 个资源、35 次 range、4913549 bytes；两条 message frame hash 均与 panel 及彼此不同。人工查看确认日文字形完整可读，没有缺字方框、横向裁剪、拉伸或旧文本残留。该证据只关闭两条可见 message E2，不代表原版像素一致。
- 公共 API 新增 `LegacyTextPresentationV1` 和 lease binding，只传递 language、显式字体 family、body/speaker region、字号、行高、行数和颜色。它通过现有 `Presentation` effect 发送，没有改动 `LegacyEffect` v1 的 postcard layout 或 ABI fingerprint。正文仍通过一次性 lease 传递，不进入 effect、snapshot、trace 或报告。Headless Host 使用现有 `CosmicTextLayoutProvider`、`TextRenderResourceOwner` 和 `astra-media-core` CPU Renderer2D，把 glyph 合成到最近的无文字 underlay；后续消息不会把上一条正文烘焙进背景。
- IDA 已确认原程序消息字体默认值为 26 px，ruby 为 12 px，默认字体是 CP932 的 MS PGothic。移植按计划显式绑定仓库内 Noto Sans JP，不读取系统字体。首轮 body/speaker region 结合已确认的 panel 几何与外部截图结构制定，真实 checkpoint 检查前只算实现绑定，不声明原版精确坐标。
- 真实原程序二进制中只有一个与已解包脚本集合相交、且没有脚本入边的 `.sc` 引用。它已作为 private Headless 实际入口，不再以先前的短链 `test.sc` 代替首路线入口。该入口的首个未覆盖命令为 `.movie`，其五个 operand 已由本地样本确认依次表达非零 movie id、资源名、宽、高和 `t`/`f` skip flag。
- `.movie` 现生成 `LegacyVideoCommandV1::Play` 和同一 media id 的 `MediaFence`，复用公共 video command、Host media completion、snapshot state 和 VFS URI 绑定；资源、尺寸、重复播放与 stage identity 不匹配均阻断。没有引入 Minori 私有播放器或 codec fallback。
- 新增 `census-movies`：默认只对每个 movie 读取有界的前 4 MiB VFS probe；显式 local-private `--full-scan` 以相同 chunk 上限遍历完整 entry。全量扫描确认五项授权 movie 都在 wrapper 后包含 MPEG start code，修正了默认 probe 的不完整结论。该命令的 report 只含 entry 数、总 decoded bytes 和格式类别计数，不写 entry 名、header、路径或内容 hash。
- 重新核对 GARbro 的 `MovPazArchive`：v1+ RC4 key 以“解码后的 entry name lower，再 CP932 编码”的字节序列构造。Rust reader 已替换此前的原始字节 ASCII lower 做法并加入 CP932 回归；真实五项的 format census 结论不变，故该差异不是当前未知容器的根因。

### 2026-08-11 路线级确定性复核

- 分支已 rebase 到本机最新 `master`。本轮没有修改八包 mount、private profile 或 key 边界。
- 原程序的 stage character 生命周期已按样本控制流闭合：`.char keep` 是只对下一次 `.stage` 生效的一次性保留标记；切换 stage 时，未标记 slot 必须退场，已标记 slot 在消费标记后保留。此前长期运行耗尽共享 texture atlas 的根因是旧实现让所有 character slot 永久存活，而不是 atlas 容量不足。runtime、provider 与 snapshot 回归均固定了该语义。
- Headless 音频采集改为只在 deterministic host 中显式打开；native realtime 不采集输出，也不改变平台音频端点。Headless 不再启动 wall-clock wake forwarder。
- Headless execution budget 现在由最后一条物理输入 tick 加所有有界 `Await.timeout_ticks` 计算，并使用 checked arithmetic。artifact 帧数和持续时间预算、标准报告的 `duration_ns` 都绑定实际执行步数，不再把等待时间误判为超限。
- fixed-tick Headless 在发出 audio command 的同一 tick 完成有界 VFS resource read；realtime host 继续异步读取。该改动消除了文件读取完成时刻跨 tick 漂移造成的 PCM 起始点差异，没有引入同步解码、系统 codec 或 fallback。
- 同一签名 plugin、mount、物理输入和 Headless profile 连续运行两次，均在 63774 fixed steps 后自然到达 terminal，消费 15032 条输入，提交并栅格化 5314 帧，生成 25 个 checkpoint 和 51018752 个 audio frame；snapshot round-trip 成立，diagnostic 为空，音频非静音。
- 两次运行的 input sequence、consumed input trace、visual trace、runtime state trace、route terminal、audio meter、submitted scene stream、rasterized frame stream 和 audio stream hash 全部一致。模型查看了全部 25 个 checkpoint；未见缺字、裁剪、拉伸、图层残影或未退场人物。截图和商业内容只保存在 ignored 私有 review artifact。
- audio artifact 仍报告 clipping。它不影响本轮确定性和非静音门禁，但在与原程序输出增益对照前不能写成音频质量通过。
- 为避免靠降低总增益掩盖问题，公共 Kira service 增加了只读 pre-master meter effect。它在 Kira 最终硬裁前累计 peak 和 overload frame，不修改 PCM。相同路线的诊断复跑仍以 63774 fixed steps、5314 帧、15032 条输入、terminal、snapshot round-trip 和零 diagnostic 通过；pre-master peak 为 1.849267，overload 为 6263 frame。首次超限时有 3 个 active stream，后续峰值发生在 3、4 或 7 个流并行时，说明 clipping 来自多流叠加，不是 WAV 量化或单个满幅样本误报。原程序的 bus headroom/limiter 行为尚未确认，因此暂不改 master gain。

### 2026-08-11 backlog 行为复核

- 当前分支再次获取远端 `master` 并执行 rebase。`origin/master` 已是当前提交的祖先，rebase 为无冲突 no-op；未提交实现和未跟踪源码在操作后完整恢复，`git diff --check` 通过。
- IDA 交叉检查 `CMessagePanel` vtable、mode setter、state 11 和 state 12。进入 backlog 时原程序不会切换 panel mode；它重新显示现有 mode 1 面板，把选中的 `CLog` 记录送入同一文本排版器。此前只绘制 `backlogGauge.png` 和 `ball.png` 的画面缺少面板与正文，已按根因修正。
- runtime state v18 保存有界的 lossless backlog、cursor 和总字节数。历史达到 16384 条、单字段 64 KiB 或合计 16 MiB 时直接阻断，不做静默淘汰。正文只存在于 local-private snapshot，并通过一次性 lease 交给 Host；公开 evidence 仍只记录计数和 hash。
- 受影响的 `astra-emu-minori` 96 个 library tests 全部通过。新增回归覆盖 message wait 挂起、滚轮打开/关闭、mode 1 panel、当前记录 lease、cursor、snapshot round-trip 和正文恢复。
- 真实八包短程 Headless E2 使用签名动态 plugin 和序列化物理滚轮输入完成 126 fixed steps、126 个呈现帧、15 条输入、3 个 checkpoint、snapshot round-trip 和零 diagnostic。人工检查确认打开页包含原版 panel、当前历史记录与滚动条，关闭后正文恢复，下一次 Enter 仍正常推进。关闭 checkpoint 需要在滚轮输入后的下一固定 tick 采样；同 tick 截图只能证明输入到达，不能证明恢复画面。
- 这次证据只关闭 backlog 的单记录显示、边界移动和关闭恢复。多条历史翻页、voice replay、配置页、显式 save/load、鉴赏页和完整标题入口路线仍未形成同一次 required-checkpoint E2。

### 2026-08-11 snapshot presentation 与 config 输入复核

- Headless restore 会按契约清空 Host 持有的临时文字与资源。Minori 过去只重发音频，恢复后的 VM 虽仍处在同一 message wait，画面却可能失去正文。provider 现在把 presentation rebind 作为 session 临时状态处理：下一固定 tick 重建当前 stage、panel、message 或 choice，成功提交后才清除 pending 标记；restore 同时销毁旧的一次性文字 lease，避免过期正文继续可取。
- 新增回归分别覆盖 message 和 choice 的 save、restore、下一 tick 资源重建与一次性 lease。标题和其他 system page 每个 tick 本来就完整重画，现也会在成功提交后清除 rebind 标记，不再把标题页的 restore 状态泄漏到第一段剧情等待。
- IDA 复核 `SystemMenuConfig` 的按键处理与 action switch：Enter 对应 apply-and-close，Escape 对应 restore-and-close；音量滑块、复选项和其他设置由独立鼠标命中区驱动。runtime 已修正标题页 config 的 Enter/Escape 返回行为，但没有把方向键猜成鼠标控件，也没有把 base 图当作完整 config E2。knob、checkmark、circle 的状态绑定和 required checkpoint 仍开放。
- 一次标题、snapshot、backlog 短程运行完成 189 fixed steps、189 个呈现帧、20 条输入、4 个 checkpoint，snapshot round-trip 成立且 diagnostic 为 0。人工检查标题和 backlog 打开画面非空，后续正文仍可推进；该序列打开 backlog 时所在的是普通 input wait，不是 active message wait，因此关闭 checkpoint 没有当前正文，不能替代此前已通过的 message-wait backlog 恢复证据。
- 首个 checkpoint 会执行强制 save/restore，下一步属于 `RestoreContinuation`。此前 Config 导航恰好与该步重叠，输入没有形成可证明的页面切换。Minori 现在通过通用 blackboard control mutation 发布 `minori.system_page`；Headless 只保留 value hash，并把该有界 observation 一并保存到 v2 resume snapshot，不把页面值或商业内容写入 report。
- 通用 `astra.emu.apply_legacy_control` action 补齐显式 Blackboard write access；未声明写入仍由 RuntimeWorld 阻断。真实标题短程复验在恢复完成后发送物理方向键和 Enter，并等待 Config observation，190 个 fixed step、190 个呈现帧、27 条输入、3 个 checkpoint、snapshot round-trip 和零 diagnostic 均通过。
- 视觉复核随后发现 Config observation 已切换而 checkpoint 仍保留标题纹理。根因是多个 system page 复用同一 texture id，同时把 archive source revision 误当作 texture binding revision。现在 revision 由 source revision 与资源 URI 共同派生，保持同资源稳定、不同资源分离；Host 因而执行严格 destroy/create，而不是静默复用旧纹理。复验已显示真实 `configBase.png` 页面。Config 的鼠标控件状态仍未实现，不能据此标记完整 Config 完成。
- IDA 对 Save/Load 和鉴赏构造函数的交叉引用补充确认：`SaveLoadMenu` 每页构造 10 个 slot record，mode `0` 选择 Save、mode `1` 选择 Load；页码限制在 `0..=9`，选择动作使用 `100..=109` 映射当前页的 10 个 slot。Save/Load 共用 base，并按 mode 选择独立标题层、selection、button、icon 和分页资源。鉴赏入口分为 CG、Flash、Music、Movie。当前只记录已确认的构造和输入契约；slot payload、命中区、自然 unlock 和页面行为仍需继续沿调用链复核，尚未进入生产 runtime。

### 2026-08-11 checkpoint、影片与 master output 复核

- 分支已获取最新 `master`；远端提交已是当前分支祖先，rebase 无冲突，未提交实现完整恢复。
- Headless 的显式 checkpoint 现在先提交待处理 Scene2D 并排空 presentation receipt。此前 Config 和 backlog observation 已切换，但截图仍可能采到上一张按间隔保留的 surface；短程真实复验中，Config、backlog 打开与关闭三组画面已经分别变化。
- decoded video 通过现有 `SceneCommand::VideoFrame` 进入 Scene2D：剧情层影片在正文前合成，modal 影片在正文后覆盖。没有增加 CPU 整帧回读或 Minori 私有 renderer。首次真实 checkpoint 依次暴露了 transient draw 只接受 Alpha blend，以及连续帧复用 deterministic transient id 时 placement 被错误释放的问题；前者在 Host adapter 修正，后者在公共 WGPU atlas 更新器修正，均未加入 family fallback。
- `Control` 长按只在脚本自己的 movie skip flag 为真时发出一次 `Stop`。首轮播放的非 skippable 分支继续等待原 media fence；回想或重播分支才能跳过。Host 关闭媒体后在下一固定 tick 完成同一 fence，family 不直接伪造 completion。
- 修正后的 local-private 标题启动完整路线完成 28814 fixed steps、2480 个呈现帧、20058 条物理输入、30 个 checkpoint、snapshot round-trip、用户 save/restore 和 terminal；报告没有 unknown/unsupported diagnostic。人工检查标题、Config、剧情、backlog、恢复点和 terminal，未见明显缺字、裁剪、拉伸或层级错误。鉴赏自然解锁和 Windows E3 仍未闭合，因此这次通过只计 Headless E2。
- 公共 Kira main track 现按 `pre-master meter -> Kira Compressor -> master-output meter` 显式组成。Compressor 使用零 attack、`-0.1 dB` threshold 和高 ratio 作为 peak limiter；pre-master 仍保留超限计数，不掩盖 family mix。定向回归用两个 `0.75` 流证明输入超过 full scale，而 master output 不超限。
- 完整路线复验观测到约 `2.71` 的 pre-master peak 和 25549 个 pre-master overload frame，但 master output peak 约为 `0.990`、overload 为 `0`、underflow 为 `0`，报告状态为 passed。完整 WAV 峰值低于 i16 full scale，RMS 非零。Headless 现在只以 master-output overload 阻断，仍把 pre-master 数据写入脱敏 telemetry 和 Perfetto counter；blocked report 写完 machine-readable 输出后返回非零退出码。
- 另一个 3028-fixed-step 真实 slice 在影片进入稳定画面后保存 checkpoint。人工查看确认 decoded frame 非空、比例正确，剧情层文字保持在影片上方，未见拉伸、裁剪、缺字或旧帧残留；该 slice 的 master output overload 与 underflow 同样为 0。网络截图仍只作外部结构参考，不用于替代该真实 runtime checkpoint。
- 原程序反编译进一步确认四个全局 clear flag：四条路线分别写入各自 flag。标题资源选择不是按文件名猜测：一个 flag 选择 `topMenu2`，另外三个全部成立时选择 `topMenu1`，其余使用 `topMenu0`。runtime 只识别这四个已确认 flag 的精确写入，并把脱敏 unlock identity 纳入 snapshot；provider 以 blackboard count 报告 session 内变化。任意带 `CLEAR` 的名称不会被猜成解锁项。global progress 的平台原子提交与新 session 恢复仍未实现，因此这些 session 证据还不能证明自然鉴赏解锁。

### 已确认事实

| 事实 | 证据等级 | 说明 |
| --- | --- | --- |
| 八个真实 index 可由同一 private profile 解开 | 本地样本 | mount preflight 与 decoded full verify 通过 |
| `bg=4616`、`bgm=49`，八包合计 14502 entries | 本地样本 | 只记录计数，不记录文件名或 payload |
| `bg.pazA` 至 `bg.pazJ` 是连续分卷 | GARbro contract + 本地样本 | 11 个物理卷完成 bounds 与 encrypted range 读取 |
| ANI/SQZ header、index 与像素 layout | GARbro contract + 本地样本 | synthetic fixture 与 6947 个真实 frame 均通过 |
| `chain`/`end` 控制流 | 原程序反编译 | `CommandChain`、`CommandEnd` 的 RTTI、vtable 和 handler 调用关系一致；普通 call 假设已撤销 |
| `set`/`setGlobal` store 边界 | 原程序反编译 | 两个 command 共用 assignment evaluator，但绑定不同 store；读取顺序为 local 后 global |
| `if`/`wait` | 原程序反编译 | `if` 固定四个 token并支持 `!=/==/>/</>=/<=`；`wait` 参数按 10 ms timer tick 递减 |
| tokenizer/`message` | 原程序反编译 + 本地样本 | 每个 space/tab 都切出一个位置，连续分隔符保留空字段；message 使用 id、voice、speaker 和拼接正文，短参数执行默认空更新 |
| `stage`/`transition` | 原程序反编译 + 本地样本 + Headless | 前景/背景/坐标/stand pair 与 VFS role 已确认；普通 PNG stage 已进入 Astra presentation，stand position 和 transition 动画仍未知 |
| `effect CrossFade2` | 原程序反编译 + 本地样本 + Headless | 已确认第一个 operand 是 effect id；真实单 operand 配置替换 primary slot 而没有资源帧。可选资源规格与数值字段尚未闭合，继续阻断 |
| `panel` mode 0/1 | 原程序反编译 + 本地样本 + Headless | mode 0 清除可见 panel，mode 1 使用默认资源；存档字段、坐标公式和 effect 上层合成已确认。mode 2–10 与过渡参数未知 |
| message 字体与 Host 路径 | 原程序反编译 + contract tests + Headless | 原程序默认正文 26 px、ruby 12 px；移植显式绑定 Noto Sans JP，并复用 CosmicText/Renderer2D；首条真实 checkpoint 已确认无缺字、横向裁剪或拉伸，不声明原版像素一致 |
| BGM/SE resource operand | 原程序反编译 + 本地样本 | token 还包含由原程序解析的资源 metadata；必须先解析再绑定 VFS |
| BGM/SE 非控制资源绑定 | 原程序反编译 + 本地样本 | 401 条引用均精确、唯一命中；`resource[volume,pan]` 在映射前剥离 metadata |
| 音频 `*` token | 原程序反编译 + 本地样本 | BGM、SE1/2/3 与 voice 均停止各自固定 stream；不是资源名 |
| 真实 Minori Headless 路线 | 本地样本 | 签名动态 plugin、八包 mount、63774 fixed steps、5314 个提交/栅格帧、25 checkpoint、15032 条物理输入、snapshot round-trip、非静音音频和 terminal 通过；同输入复跑的 VM、画面、音频与 terminal hash 全部一致。另有短程物理滚轮 E2 验证 backlog panel、当前记录和关闭恢复 |

### 冲突与 blocker

- 旧文档把六个业务包误写成完整集合。八包 full verify 已补齐：14502 entries、43818 次 range read、6624958365 个 decoded bytes。
- 启用 cache 的八包验证因平台私有缓存卷空间不足，在首个写入处阻断。no-cache full verify 已通过，但 cache identity 第二轮全命中仍需单独证据。
- 首次八包挂载需要读取约 5.74 GiB source 并为 entry 建立完整性身份。旧实现先顺序哈希全包，再逐 entry 随机重读 encrypted range；现已改为一次有界顺序流同时计算 source 与 entry hash，并阻断重叠 range、短读和期间的 metadata drift。跨分卷、零长度和 overlap 回归已通过。相同本地样本的 mount 区间由约 466 秒降至两次单流运行的约 367 秒和 403 秒；各 role 的 `archive_hashed` 后均立即完成 mount。机械卷吞吐仍占主要成本，因此只记录约 14%–21% 的实测改善，不宣称秒级启动。
- `stage` 已覆盖无 stand 的普通图像路径，但 stand position 不能按名称猜成像素坐标；遇到 stand 时返回稳定 blocker。transition 配置已保存，动画插值和 fence 尚未接入。
- 真实 Headless 已完成当前入口的整条剧情路线，并检查 25 个选定 checkpoint；这仍不是与原程序逐帧节奏或像素一致的证明。普通 voice、其他 panel/effect、系统页和后续路线仍需逐项确认；assignment 的字符串值和除零行为也仍需脱敏 census。
- AstraEMU runner 同时输出专用 `astra.emu.headless_run_report.v2` 和公共 `astra.headless_run_report.v2`；两者绑定同一 manifest hash、输入、checkpoint 和 diagnostic 状态。真实八包 v24 已通过 `prepare-review`，模型按 bundle 查看 6 个 required checkpoint、首尾/最大差异选择，并检查 3 个完整 WAV 的时长、电平、静音与 clipping；`validate-review` 随后通过。该 review 只适用于 373-tick slice，不能覆盖完整路线或自动失败。
- 当前入口的首条剧情路线已有 terminal E2 和重复运行确定性证据，backlog 也有独立短程 E2；required checkpoint 仍未在同一次标题启动路线中覆盖配置、显式 save/load 与鉴赏，也没有 Windows E3 证据。
- 实际入口的首个 movie 为约 196 MiB，超过 family VFS 单次 64 MiB read 上限。Headless 现改用有界 range-backed reader，并在 wrapper 中逐块扫描 MPEG start code；当前不再触发 `ASTRA_EMU_VFS_RUNTIME_RANGE` 或以固定前缀错误判为未知容器。仍需由 decoder 的实际 frame/audio 输出验证 container 与 codec，真实路线在 movie fence 处保持 blocking。
- 原始 movie extension 不是容器证明。私有全量 census 确认五项均为 `mpeg_wrapped`，但 start code 只说明候选 MPEG 流的偏移，不等于完整解码或播放。runtime 只会以相同有界 offset 交给显式 pure-Rust provider；解析、格式、时间线或 media fence 任一失败均阻断，不得使用 FVP decoder、平台媒体 API 或伪造 completion 继续路线。

### 本次测试

```sh
cargo test -p astra-emu-family-core -p astra-emu-minori -p astra-emu-minori-cli
cargo test -p astra-emu-family-api
cargo test -p astra-emu-minori -p astra-emu-cli --lib
cargo test -p astra-emu-fvp -p astra-emu-manager-core --lib
cargo test -p astra-platform-headless --test host_contract surface_without_a_submission_can_be_destroyed
cargo check -p astra-emu-cli
cargo clippy -p astra-emu-family-api -p astra-emu-minori -p astra-emu-cli -p astra-emu-fvp -p astra-emu-manager-core --all-targets -- -D warnings
python Tools/check_docs.py
```

此外，ignored 私有样本完成一次签名动态 Minori Headless E2：373 fixed tick、9 个实际呈现帧、6 个 checkpoint、snapshot round-trip 和音频 artifact。自动报告为 passed，消费 16 条物理输入，diagnostic 为空。模型按公共 review bundle 查看黑场、居中竖排标题、可见灯光 effect、底部 panel 和前两条日文正文，并检查三条完整 WAV；`validate-review` 通过。画面未见缺字方框、横向裁剪、拉伸或旧文本残留，音频无 clipping。这仍不代表完整 effect 周期、完整 VM、完整路线或 Windows E3 完成。

2026-08-11 的路线级复核使用同一组序列化物理输入连续执行两次。两次均完成 63774 fixed steps、5314 个提交/栅格帧、15032 条输入、25 个 checkpoint、51018752 个 audio frame、snapshot round-trip、terminal 和零 diagnostic；九项输入、VM、scene、raster、audio 与 terminal identity 全部一致。模型检查全部 checkpoint，未见缺字、裁剪、拉伸、图层残影或人物生命周期泄漏。audio artifact 为非静音，但 clipping 标志为 true，因此只关闭确定性与可听性，不关闭音频质量对照。

### 下一步

1. 按原程序的四路线 clear gate 完成后续路线，补齐 `Memories`、CG、BGM、Flash/回想与 Movie 页面 required checkpoint；首路线不会人为解锁该入口。
2. 完成 Config 鼠标命中区、Save/Load slot 页面与 voice replay 的真实输入 E2。
3. 在具备足够空间的私有缓存卷复核 cache identity，并补齐 Manager media preview、Linux FUSE 与 macOS extract 证据。
4. 使用同一 build/profile/package/input identity 进入 Windows Manager E3；在此之前不把 Headless E2 写成平台完成。

### 2026-08-23 ABI v9 Host bridge 与 surface ownership

- 当前分支已包含修正后的 ABI 基线 `3f9a771ac`。动态 family loader 现在接收完整 `LegacyFamilyHostServicesV9`，FFI 回调逐项转发 VFS、可写 surface、同步 Hook 和 writable-file；create 失败或 instance 销毁时会移除整组 Host services。旧 VFS-only token 不再保留。
- `astra-emu-family-support` 新增公共 surface store。它在 lease 发出期间移走 retained allocation，commit 后收回同一 allocation；generation、session、fixed step、尺寸、格式、单 surface 配额和总配额均严格校验。定向测试确认 Rust 路径的指针在 commit 和下一次 acquire 间保持不变，并覆盖重复 lease 与错误 fixed step。
- Manager runtime adapter 已删除 v9 不再提供的 Scene2D、ephemeral text、session resource 和 family snapshot 调用，并开始把 family `Layer2D` transaction 映射到 Product live output。同步 Hook 取代 provider completion；family 若仍提交旧 completion wait 会返回 `ASTRA_EMU_PROVIDER_COMPLETION_REMOVED`，不会构造空 payload 继续运行。
- Product host 会按 descriptor 阻断 presentation lane 混用。`Scene2D` provider 不能提交 Layer2D；`Layer2D` provider 不能提交旧 scene 或 ephemeral-text payload。
- 当前证据仅包括 `astra-emu-manager-core --lib` 编译、surface store 2 项测试、`astra-plugin-abi` 单元测试、Product host 定向回归和 Concurrent host 3 项测试。FVP 仍保留待迁移的 v7 scene/text/save consumer，导致 CLI 与 Manager 的完整测试目标无法编译；Minori text surface、translation Hook、writable-file composition 和新的 Headless E2 也尚未完成。因此 ABI v8 的路线证据继续只作历史基线。

### 2026-08-12 结局返回标题与 choice 视觉复核

- `origin/master` 的最新提交已是当前分支祖先，本轮 rebase 为 no-op；工作树中的实现与私有研究产物未被覆盖。
- 原程序反编译确认 `.end` 在标题启动 session 中回到 `SceneMainMenu`。runtime 现区分 title launch 与 direct entry：前者清理剧情瞬态状态并重建标题，后者保持 terminal 语义。返回标题后再由物理方向键和 Enter 选择 Exit，不能把黑场或 `.end` 本身当作应用退出。
- 原程序标题资源和菜单 gate 已按四个已确认 clear flag 实现。首路线只自然写入其中一个 flag，标题仍使用普通四项菜单；`Memories` 只有在原程序要求的后续 clear 条件满足时出现。CG、Flash、BGM 的静态入口资源已按原程序构造关系绑定；Movie 页面依赖脚本驱动，未实现时保持稳定 blocking，不生成替代页。
- 真实脚本 slice 以严格 `minori.choice_active` blackboard observation 捕获首个 choice，并把 `post-choice` 延后到分支已进入实际剧情画面后再取证。运行完成 156 fixed steps、290 条输入，snapshot round-trip 成立、diagnostic 为空；人工检查选项资源、选中态、日文字形、背景合成和提交后的剧情画面均正常。该选项帧的 raw RGBA hash 也命中完整路线 fixed step 13928，证明 slice 与完整路线观察到同一画面，不把相似截图当成关联证据。
- 同一签名 plugin、mount、Headless profile 和物理输入连续执行两次完整路线。两次均为 31011 fixed steps、16947 条输入、31627 个提交/栅格帧、31 个 checkpoint、25277440 个 audio frame；snapshot round-trip、用户 save/restore、自然 unlock、结局返回标题和最终 Exit 均通过，diagnostic 为空。
- 两次运行的 VM state、visual trace、route terminal、coverage、audio meter、submitted scene、rasterized frame 和 audio stream hash 全部一致。local-private session id 会进入 input sequence 与 consumed input trace，因此这两项 hash 按设计不同，不把它们误写成跨 session 一致。
- master output peak 为 0.989551，overload 与 underflow 均为 0。人工查看标题、Config、首条消息、转场、多人构图、结尾和返回标题 checkpoint，未见缺字、裁剪、拉伸、图层残影或影片比例错误。该结论只关闭 Headless E2；Windows E3、Config 控件、完整 Save/Load UI 与四路线后的鉴赏页仍开放。
- 完整路线与 choice slice 均已生成 `astra.headless_review_bundle.v2`。模型查看了 bundle 要求的全部 frame；完整路线 WAV 为 48 kHz 双声道、526.613 秒，peak 0.989532、无 full-scale sample，左右声道 RMS 差 0.005 dB；choice slice WAV 为 2.592 秒，peak 0.980255、无 full-scale sample。当前环境没有完成涉及语音内容的整段试听，因此最新 review v3 以绑定 WAV hash 的 typed artifact verdict 记录 `ASTRA_HEADLESS_REVIEW_AUDIO_LISTEN_PENDING`，`validate-review` 按预期返回 `ASTRA_HEADLESS_REVIEW_BLOCKED`。不得把视觉检查或自动音频量测写成正式 review 已通过。
- 尝试在同一次标题启动运行中追加 choice checkpoint 时，严格 observation 已通过，但第一选项分支在原 4200 次输入预算结束后仍停在一个输入等待，随后 `route_complete` 等待按预算阻断。失败 run 已清理且未作为 evidence；当前仍以两次完整路线自动证据加独立 choice slice 作为分层证据，不能宣称“同一次完整路线已包含 choice checkpoint”。

### 2026-08-12 动态立绘与 GPU 性能门禁整理

- 本轮再次同步 `origin/master`，远端最新提交已是当前分支祖先；rebase 没有改写现有提交，autostash 完整恢复了未提交实现。
- 原程序 `CCharLayerManager` 的 parser 与 executor 交叉确认了 `.char trans`：参数依次是 slot、毫秒持续时间和目标透明度。执行时保持当前位置，透明度按当前值到目标值线性插值；命令等待原生 layer 的 transition flag 清除后才结束。`.char vis` 使用原程序既有的首字符布尔解析规则。生产实现只开放这两种已确认语义，不增加呼吸、文件名推断或 ANI 自动播放。
- runtime state 硬切为 v22。每个活动 transition 保存起始/目标透明度、持续时间、已过纳秒和完成位；同一时刻只允许一个由命令阻塞的 character transition。provider 在等待期间持续提交 retained Scene2D 更新。定向测试覆盖 50% 中间帧、最终帧、等待 token、非法时长/透明度、snapshot continuation 与 provider 合成。该 title 的 290 条真实 `.char` 只包含 87 条 `load`、87 条 `pos` 和 116 条 `keep`，所以这些测试属于 E1，不冒充真实脚本动态立绘 E2。
- 既有完整路线 artifact 已核对为 `wgpu_offscreen`、DX12 集显，而不是 CPU reference；该次运行提交并栅格化 34172 帧。它证明真实路线走过 native GPU retained scene，但没有正式 performance budget、固定采样窗和 Perfetto identity，不能关闭 GPU 性能验收。
- CLI 的通用性能路径仍把 trace workload 写死为 `fvp.real_game.120hz`。现已改为显式 family identity，并提供 `prepare-headless-performance-profile` 生成可复用 budget profile。正式 Minori run 仍需在 clean Release 身份下完成精确 36600 fixed tick、73200 presentation、1200 帧 warmup、72000 帧测量，并生成共享 report 和 trace manifest；这项证据尚未生成。

### 2026-08-13 retained scene、动态立绘视觉证据与性能复测

- 首轮正式采样暴露标题页每个 fixed tick 都重新读取、解密并解析同一张 PNG。Minori provider 现只在首次呈现、系统页输入变化或 restore 时重建 resource scene；未变化页面沿用 Host retained Scene2D。runtime fixed tick p99 从约 41.7 ms 降到 0.3491 ms。该修复没有增加 cache、fallback 或 family 私有 renderer。
- 快速 retained scene 又暴露 Headless 性能路径只轮询 completion、没有在平台有界队列前施加背压。通用 runner 现于正式性能采样逐帧等待最老 GPU fence，非性能产品路径保持原行为。CLI 定向测试和 all-target clippy 通过。
- 同一 clean Release identity 完成 36600 fixed tick、73200 presentation、1200 帧 warmup、72000 帧测量，并生成 Perfetto、`astra.performance_report.v1` 和 trace manifest。runtime p99 为 349100 ns，presentation p99 为 924060 ns；内存、增长、稳定段上传/readback/allocation、音频 underflow、trace dropped 和 scene full resync 均通过。仍有 2 次 presentation 超过 8333333 ns，`deadline.miss_count` 因零容忍预算返回 blocking。该结果不能写成正式性能门禁通过。
- 动态立绘使用合成 `.char load/.char trans` 脚本和非商业纹理生成 256、128、0 三个透明度状态，并通过 Headless `wgpu_offscreen` 实际 capture。三张 PNG 的 SHA-256 分别为 `9ca68aeb43df0e0f48ac6689e55abd78f64ac766a3e53f149077b5ffe04229b6`、`a18cbfecb37f601bc969c77a4144629ec082271412d17745c343782a5ebc6c78`、`41ee454a1fcaf892305c58c474fa7808e681af8cb0526a3c7f50ed9a8135e9aa`。人工检查确认全显、半透明和完全消失，没有裁剪、残影或混合异常。真实 290 条 `.char` 仍只有 `load/pos/keep`，所以这项关闭的是合成动态立绘 Headless E2，不是授权样本路线覆盖。
- 复核后发现上一轮 `EmuHeadlessGpuObserver::pace_gpu_frame` 是空实现，73200 个 presentation 没有按 120 Hz 墙钟节拍提交。该报告只保留为诊断数据，不再作为正式性能证据。observer 现按 presentation sequence 计算有理数 deadline；发生 authored idle 时从当前时间重建原点，不追赶已经错过的 presentation。
- 修复 pacing 后，同源签名 clean Release 完成 1200 帧 warmup 和 72000 帧测量。runtime p99 为 320900 ns，presentation p99 为 907240 ns；内存、增长、上传/readback/allocation、音频 underflow、trace dropped 和 full resync 均通过。8 次 deadline miss 使报告保持 blocking，最大 presentation 为 313301860 ns。
- 为排除 trace 扰动，高频 phase/counter 改为每 60 fixed tick 记录一次；完整逐帧性能 sample 和全部 GPU flow 仍保留。提前终止的下一轮在前几分钟已出现 6 次不可逆 miss。5 次来自 `scene_gpu_ns` 的 8.14 至 9.92 ms 抖动，另一次是 `cpu_submit_ns` 437.623 ms，而 scene build、atlas upload、filter、资源上传和 runtime tick 均正常。当前证据指向集显/驱动调度长帧，不支持继续修改 Minori VM 或放宽零 miss 门禁。

### 2026-08-22 正式 GPU E2 收口

- 独立 Scene2D 对照运行先在相同集显和驱动上完成 1200 帧 warmup、72000 帧测量，deadline miss 为 0，确认共享 WGPU renderer 与测试环境能够满足 120 Hz 预算。
- 产品路径的长帧最终定位为过大的 timestamp query 在途窗口触发 `wgpu::Queue::submit` 周期性背压，而不是 Minori VM、scene build 或 GPU draw 超限。Headless performance observer 现在显式声明最多两个 GPU frame profile 在途，PlatformHost 会校验该值落在 query ring 的安全范围内；缺失或越界直接阻断。正式调度沿用平台公共 scheduling guard，renderer 与 driver identity 从 artifact manifest 读取，不再由 family 字段代填。
- 同一 clean Release build、package、profile、物理输入、adapter 与 driver identity 连续完成三次正式运行。每次均执行 36600 fixed tick、73200 presentation，其中 1200 帧 warmup、72000 帧进入十分钟测量窗；deadline miss、audio underflow、scene full resync、trace dropped、稳定段 upload/readback/allocation 与 memory growth 都为 0，trace 未截断。三次 runtime p99 分别为 0.2841、0.2725、0.2938 ms，presentation p99 分别为 1.11064、0.97864、0.81456 ms，最大 presentation 分别为 6.8416、7.81664、3.51376 ms。正式 Minori GPU 性能 E2 至此通过。
- 该负载使用真实八包、签名动态 provider、VFS、RuntimeWorld、retained gameplay scene 与七条序列化物理输入，但停留在静态标题场景，未到 terminal，音频为静音。它只关闭持续 GPU 提交与运行时预算，不替代完整路线、影片/音频、正式视觉 review 或 Windows E3。

### 2026-08-23 Family ABI v9 可写 surface 迁移

- 分支已基于 ABI v9 可写 surface 修正 `3f9a771ac`。Minori 使用共享 `FfiLegacyFamilyHostAdapter` 获取 VFS、surface、Hook 和 writable-file 四个 Host port；不存在 immutable surface、复制回退或 const-cast。
- resource scene 已进入 ABI v9 的 `Native + MultiLayer` 主路径。family 按已确认的 texture id 域拆分 background、foreground、effect、panel 四层；未知 id 直接返回 `ASTRA_EMU_MINORI_LAYER_CLASSIFICATION`，不按资源名猜测。
- 图像继续由 `image` 解码，四层栅格复用 `astra-media-core::CpuRendererProvider`。输出按 Host lease 的 stride 写入独占 `OwnedWritableByteBuffer`，提交 full damage 后生成 retained `LegacyLayerTransactionV9`；没有 Minori 私有 rasterizer、surface 镜像或 ABI payload copy。
- 119 个既有 Minori library tests 已通过。旧 provider-result storage 用例已经删除，改为验证同步 writable-file 的原子写 round-trip。新增用例覆盖四个 exclusive Host surface、带 padding 的 stride、premultiplied RGBA、full damage 与四层 create transaction。同步 translation Hook 和 CosmicText text surface 尚未迁移；实际文字呈现会返回 `ASTRA_EMU_MINORI_V9_TEXT_NOT_MIGRATED`，随后毒化 session。因此，历史 ABI v8 Headless 路线只作行为回归基线，本轮不能声明新的 E2。

### 2026-08-23 ABI v9 consumer 与文字 surface 后续

- 当前基线更新到 `e6bc3d960b87373160acd8507faeac4cc589975b`。Layer2D 的滤镜字段使用 ABI-owned `LegacyFilterGraphV9`，consumer 不再接收字符串 binding，也不会按名称或 hash 解析 graph。RFVP 依赖固定为 `15d6c1f9fa490f0d1d87a58dda601ca276ccd8f9`。
- CLI 已组合 VFS、Host-owned surface、同步 Hook 和安全相对路径 writable-file 四个 port。Layer2D transaction 会校验 session、sequence、generation、damage、stride 和 typed filter graph，再交给 Astra Renderer2D；旧 scene、text lease 和 session-resource 路径只返回迁移错误。CLI library 38 项测试和严格 clippy 通过。
- Manager 已绑定同一组四个 Host port，并增加显式 session Hook router。translation companion 现在同步返回 `Completed`、`Unbound`、`TimedOut` 或 `Failed`，避免在 framebuffer acquire 后等待翻译。Manager 的 retained Layer2D 可以读取已提交 surface、处理 BGRA/stride/premultiplied alpha 并提交 WGPU。per-layer typed filter graph 会先经过公共 `FilterValidator`，再由 WGPU 顺序执行 bloom、color-matrix 和 fade；参数、节点或 kind 不符合公共 contract 时阻断，不按字符串查找 graph，也不切换 CPU executor。WGPU validation test 覆盖三类节点。旧 `render_scene_live` 与相关 scene-resource transaction consumer 已删除；当前仍没有 ABI v9 产品 E2，因此只计迁移中的 E1。
- Minori message path 已移除 `ASTRA_EMU_MINORI_V9_TEXT_NOT_MIGRATED`。speaker 与正文先进入同步 translation Hook，再由 family-owned CosmicText、打包的 Noto Sans JP 和 Astra CPU Renderer2D 生成 `minori.surface.text`，最终以 z=400 的 retained text layer 提交。定向测试确认 Hook 发生在首个 surface acquire 之前，并覆盖资源层与文字层的 exclusive lease/commit。
- 公共 support 新增私有 writable-file Host 和 surface 像素规范化。Unix 权限、原子 replace、范围限制、路径穿越与 symlink 阻断已有测试；Windows 复用 cache 的 protected owner-only DACL，但尚缺“owner 必为当前用户”的独立断言，不能把 writable-file 安全门禁写成完整。support 25 项、Manager Hook 2 项及 Minori v9 2 项定向测试通过。真实样本尚未在 ABI v9 下重跑，历史 v8 Headless 结果仍只作回归基线。

### 2026-08-22 Config 动作与状态层

- 本轮 rebase 前后都确认 `origin/master` 已是当前分支祖先，没有产生冲突或改写既有提交。
- IDA 复核 `SystemMenuConfig` 构造、命中函数和 action dispatcher，确认 29 类动作、六条滑块公式、Apply/Cancel 语义和 `configBase.png`、`knob.png`、`checkmark.png`、`circle.png` 四类资源。研究记录只保存函数地址、字段语义和尺寸，不保存商业图片或私有路径。
- runtime state 硬切到 v23。Config 使用独立 draft，Apply 原子提交，Cancel 丢弃修改；页面内 snapshot/restore 保留 draft 和指针状态。provider 只接受物理 pointer/Enter/Escape，使用原坐标命中区，不提供方向键替代。四类资源经 retained Scene2D 组合，尺寸漂移直接阻断。
- BGM/Voice/SE 音量与静音在公共 audio command 边界计算；试听资源显式标为 WAV，离开 Config 时停止专用试听 stream，不把一次性试听带进 restore continuation。114 个 library tests 和目标 crate clippy 通过。这是 E1：真实资源 Headless 视觉、全屏 Host effect、逐字速度、视觉开关和角色语音筛选仍开放。
- 首次真实试听严格阻断于资源查找。VFS 清单确认 archive identity 是大小写敏感的 `BGMTest.wav`；随后又发现三个原生 SE bus 分别为 `se`、`se2`、`se3`，都应受同一 SE 配置控制。实现按已确认 identity 修正，不加入大小写搜索或未知 bus fallback。
- 修正后的签名 Release plugin 在真实八包上完成 Config 短程 Headless E2：166 fixed steps、171 个提交/栅格帧、42 条物理输入、6 个 checkpoint、snapshot round-trip 和零 diagnostic。默认页、关闭画面效果、BGM 滑块 50% 和试听 checkpoint 的状态 hash 均按输入变化；完整 WAV 有 132608 frame，peak -3.5906 dBFS、RMS -17.8713 dBFS，无静音、clipping、master overload 或 underflow。
- 模型查看全部 6 张 checkpoint。checkmark 与 knob 对齐原版控件，开关移除和滑块移动清晰可见；标题及进入剧情后的画面也没有裁剪、拉伸、错层或残留。视觉 verdict 通过。完整试听仍未由具名人工完成，所以 v3 review 以 `ASTRA_HEADLESS_REVIEW_AUDIO_LISTEN_PENDING` 返回 blocking；该阻断不被模型视觉结论覆盖。

### 2026-08-23 ABI v9 媒体与 typed observation 恢复

- 当前分支基于 AstraEMU ABI v9 implementation `635527831e89e5ff9b87ac165b5b5532e28356c6`，RFVP 固定为 `f4f64a5bb726c1759350a666a35e0a454b810f61`。Minori 继续使用 `Native + MultiLayer`；surface 是独占可写 lease，FilterGraph 使用 typed graph，未恢复旧 scene、text lease、family snapshot 或 session-resource ABI。
- v9 真实八包短程启动已通过签名动态 plugin、VFS、RuntimeWorld、Layer2D、CosmicText 和音频生命周期。该 run 的自动结果为通过，但最早的标题 checkpoint 发生在首张有效标题帧之前，后续 Config 标签也没有与页面变化严格对齐，因此人工视觉结论保持 blocking，不能沿用历史 v8 E2。
- 完整路线首先暴露 `LegacyVfsReader::read_file` 把约 196 MiB 影片作为一次 range 请求，而公共 byte-source transport 的单次上限是 16 MiB。公共 whole-file helper 现按该上限分块，逐块校验 source revision、返回 range 和短读；调用方总预算不变，不放宽 transport 上限。
- （历史记录，已被后续实现取代）当时 Minori 影片主路径曾绑定仓库内的 `AviDemuxer`/`Wmv3Decoder`；该方案不再属于当前生产路径。
- 当前生产路径已经删除 Minori 对 `wmv-decoder` 的依赖，统一由 AstraMedia 的显式 `ffmpeg-vcpkg` 增量 provider 负责 AVI demux、WMV3 decode、PCM resample、PTS 校验和取消；缺 provider 或格式不符直接返回 blocking diagnostic，不回退到手写或平台 codec。
- Headless await 不再接受 runtime semantic hash。当前只支持 bounded typed existence marker；Minori route 使用 `choice_active`、`route_complete` 和自然 unlock marker。`continue_at_match` 只扣除 await 已预留但未消费的 timeout tick，不跳过 VM、媒体或 presentation tick。真实纯 Rust影片 run 已到达首个 choice marker；完整路线、带 checkpoint 复跑和正式 visual/audio review 仍在进行，不能计作新的 ABI v9 E2。
- 首次完整诊断在 `choice_active` 之后被 Headless 音频 artifact 时长门禁阻断。根因是 host profile 只按最后一条消息的 tick 估算执行上界，漏掉了未被后续 tick 覆盖的 Await 和 `AdvanceTicks`；不是影片 PCM 重复提交。输入验证现在按消息顺序递推最坏执行 tick，artifact 预算使用该上界，标准报告则记录实际 fixed step。清理阶段的 resource leak 只作为音频根错误后的级联结果保留，不单独结案。
- 修复 artifact 上界后的第二轮诊断越过原阻断并完整推进剧情，但以 `ASTRA_EMU_HEADLESS_AWAIT_TIMEOUT` 结束。代码复核确认 CLI 虽接收 `--entry`，却没有传递 title/direct 启动语义，Minori 因而按 `DirectEntry` 在脚本末尾进入 terminal，不会生成返回标题的 `route_complete`。通用 CLI 现增加显式 `--launch-mode direct|title`；Minori composition 将其映射到既有 family profile，其他不支持 title 语义的 family 会直接阻断，不静默忽略。

### 2026-08-25 ABI v9 当前签名包 E2 复验

- 当前 consumer 分支已直接 rebase 到 `origin/codex/astraemu-layer-hook-v9` 的 `635527831e89e5ff9b87ac165b5b5532e28356c6`，没有保留旧 ABI 或兼容 shim。签名 development package 使用当前 workspace 的构建身份生成；package、profile 和 artifact identity 只在忽略的本地目录保存。
- 当前 package 的 Minori `Native + MultiLayer` 路径在真实八个 archive role 上完成完整首路线 Headless 运行：`astra.emu.headless_run_report.v3` 为 `passed`，`25499` 个 fixed step、`13170` 个 presented frame、`25` 条物理输入、`7441` 个 coverage id、`terminal_reached=true`，diagnostic 为空。VFS 统计为 `2338` 个 resource、`2442` 个 unique range、`10809` 次 read 和 `7923100943` bytes；这证明分块后的纯 Rust PAZ/AVI/WMV3 路径能够承受真实长影片读取，不把 16 MiB transport 上限放宽到单次大读。
- 同一 package 的 Config 短程运行通过：`168` fixed step、`122` presented frame、`42` 条输入、6 个 checkpoint、零 diagnostic。模型查看了标题、Config 默认页、关闭画面效果、BGM 音量变化、BGM 试听和进入剧情后的画面；控件、文字、背景、层次、比例和透明度没有发现裁剪、拉伸或残影。音频 artifact 为 48 kHz 双声道、`134144` frames，量测 peak `32419`、RMS `8402.36`（原始样本值），但未完成具名人工试听。
- 同一 package 的 backlog 短程运行通过：`771` fixed step、`30` presented frame、`55` 条输入、6 个 checkpoint、零 diagnostic。模型查看了打开 backlog、向上翻页两条记录、关闭 backlog 和返回剧情的 checkpoint；滚动条、记录顺序、消息层与关闭后的场景切换均可见，未发现空白替代或图层残留。音频 artifact 为 48 kHz 双声道、`616448` frames，量测 peak `32405`、RMS `4391.58`；同样只作为自动量测，不作为人工听感通过。
- 当前完整路线输入没有插入 checkpoint 事件，因此不能把 terminal 运行误写成完整视觉 review；title timeline、Config 和 backlog 的 checkpoint 已在当前 build/package identity 下人工查看。影片播放、首选项/跳过、存档恢复、结局返回标题、CG/BGM/回想页的完整 checkpoint 集合及 Windows E3 仍未关闭。历史 ABI v8 review 不跨 ABI 继承，新的正式 `prepare-review`/`validate-review` 仍需在具名人工音频试听完成后运行。
- 本轮只运行受影响 crate 的增量测试、`cargo build -p astra-headless`、`cargo fmt --check`、目标 clippy 和 `Tools/check_docs.py`；没有用全 workspace 测试替代当前证据，也没有把忽略目录中的商业 payload、截图、音频或私有路径写入仓库。

### 2026-08-25 鉴赏页与真实缩略图契约复验

- 使用当前源码重新生成的签名 package 完成鉴赏页 Headless E2：`82` 个 fixed step、`12` 个 presented frame、`64` 条物理输入、`9` 个 checkpoint、零 diagnostic。VFS 统计为 `45` 个 resource、`45` 个 unique range、`111` 次 read 和 `31046383` bytes；报告只保留 family、计数和 identity，不写文件名、路径或商业内容。
- 复核输入语义后修正了鉴赏序列：在 BGM 页按 Escape 会回到 Memories 的 focus 0，随后 Enter 才是 CG；旧序列把同一张画面误标成 movie。修正后的序列实际覆盖 CG 第 1/2 页、回想页、BGM 播放和 movie 列表页，所有 checkpoint 已逐张查看。
- `sys/cgthumb` 的真实缩略图尺寸由样本确认是 `128x72`；provider 现在对该尺寸做严格校验，错误尺寸阻断，不缩放猜测。CG 页按已验证的 4x4 网格呈现，最后一页的空槽保持资源定义的空态。
- 标题、Memories、BGM、CG 和回想页面的文字、比例、层次和焦点没有发现裁剪、拉伸或残影。movie 列表已使用真实标签与脚本目标校验，但当前样本没有独立的 movie gallery 背景资源；运行时保留已验证的 Memories 背景和有界文字层，不能把它写成原版逐像素一致。实际影片播放仍由剧情 movie fence 证据覆盖，鉴赏页的原版视觉 parity 继续开放。
- 该次鉴赏运行使用 local-private global progress 进入页面，不能证明四条路线自然解锁或完整鉴赏解锁条件；正式人工音频听审、自然 unlock 全量证据和 Windows E3 仍未完成。自动非静音量测也不替代具名人工听审。
- 复核页面返回路径时发现 BGM gallery 的 Escape 会回到 Memories，却没有停止共享 BGM stream；这会把鉴赏音频泄漏到父页面。runtime 现把该返回动作建模为 `GalleryBgmStop`，先停止 looped stream 再提交 Memories，pointer Stop 与键盘返回共用同一条音频边界；新增 provider/runtime 回归后 Minori library 为 `130/130`。

### 2026-08-25 配置持久化与媒体恢复边界

- 当前 consumer 在 `6f554012b` 收紧保存槽恢复：v9 `LegacyAudioCommandV1::Play` 与 `LegacyVideoCommandV1::Play` 没有 seek 起点，载入包含非零 `continuation_pts` 的槽现在在加载边界直接返回 `ASTRA_EMU_MINORI_MEDIA_CONTINUATION_UNSUPPORTED`，不会先替换 VM 再在下一 tick 伪造续播或隐藏失败。已有 presentation/restore 保护保持不变。
- 配置写入继续使用 identity-bound `astra.emu.minori.config.v1` envelope、temporary file + range write + SetLength + atomic replace；相同配置不会产生第二次写入。Minori library 定向回归为 `133/133`，clippy、fmt、文档检查和 diff 检查通过。
- 这项修复不扩大 ABI、不开启平台播放器或私有 seek API；需要原位置媒体恢复仍必须先有经过验证的公共 Host/media contract。完整四路线、正式音频听审、movie gallery 原版视觉 parity、cache volume、Linux FUSE 和 Windows E3 继续保持 blocking。

### 2026-08-25 v9 短程 Headless 视觉复验

- 在 `6f554012b` 后用当前签名 package、mount profile 和序列化物理输入执行了受限 Minori route smoke：107 个 submitted/rasterized frame、85,504 个音频 frame、68 个输入序列事件、2 个 checkpoint、diagnostic 为 0，Headless report status 为 `passed`。该运行没有到达 terminal，也不替代完整路线报告。
- 实际查看两个 checkpoint：背景资源和日文正文均非空，字形、比例、透明度、层次和画面边界正常，未见拉伸、裁剪或残留层。该检查只证明当前 v9 surface/Layer2D/text/media 组合的短程可观察输出，不能证明原版像素 parity、影片续播、四路线自然 unlock 或 Windows E3。

### 2026-08-25 脚本资源引用审计

- Minori runtime 新增显式 `astra.resource_audit=full` policy；通用 CLI 的 `--audit-all-resources` 仅在 `--family minori` 时注入该选项。open 阶段通过 bounded VFS enumeration 读取并解析全部 `.sc`，复用执行路径的 token、stage/character/effect/audio/movie/panel/chain validators，逐项检查资源存在、非空和 `1 GiB` 上限。
- 审计只保留脚本/资源 URI identity、长度、revision、计数和聚合 digest；没有目录遍历、商业 payload、key 或本地路径进入日志、snapshot 或 report。缺少枚举能力、脚本资源缺失、源 revision 漂移、短读和未知已确认形态继续返回 blocking diagnostic，不做猜测或 fallback。

### 2026-08-25 消息 read identity

- runtime snapshot 硬切到 `astra.emu.minori.runtime_state.v24`。每次 message wait 由物理输入、Auto timer、Control/Skip timer 或 await completion 结束时，记录由当前脚本 hash、source span、message id 和正文 hash 组成的 read identity；identity 以排序 bounded vector 保存并参与 snapshot/state hash。
- 该改动只固化“已确认消息”的持久状态，不猜测 `messageSpeedTBR`/`messageSpeedRead` 的逐字 reveal 公式，也不把 skip 的原版未读策略提前写成事实。重复文本在不同脚本 revision 或 source span 不会互相标记；损坏、重复、无序和超限 identity 在 restore 时阻断。
- 新增 runtime grammar census 与 provider VFS audit 回归；目标 crate 测试、clippy 和格式检查在提交前复跑。该门禁只证明资源引用覆盖，不替代完整路线、codec、人工音频 review 或 Windows E3。

### 2026-08-28 当前运行时接线修正

- Manager 的 quick launch 现在接受显式 `ASTRA_EMU_QUICK_ENGINE=minori`，并校验 `ASTRA_EMU_QUICK_ENTRY` 的 canonical `minori:/...` URI。此前入口只允许 FVP，导致 Sandbox 的真实 Minori 窗口在扫描后被静默阻断；该路径已改为显式 family 选择，不按注册顺序或 case 名称猜测入口。
- `census-scripts` 升级为 `astra.emu.minori.sc_census.v5`。逐文件结果只包含稳定序号、解码大小、源字节 SHA-256、行/命令/opcode 计数和 unknown 计数，正文、operand、label、跳转目标、URI 和 key 仍被排除。该报告用于定位 parser/runtime 覆盖，不构成商业脚本导出。
- 本轮增量测试覆盖 `astra-emu-minori-cli` 13/13 与 Manager 3/3；Windows Sandbox 尚未形成可回收的音频/artifact E3，继续保持 blocking。
### 2026-08-25 Minori AVI 输入与帧预算收紧

- 当前 `MinoriAviDecodeProvider` 在容器解析前拒绝空输入和超过 512 MiB 的预览请求；该边界与公共 viewer 的媒体预览预算一致，避免直接 provider 调用绕过 viewer 预算。超限固定返回 `ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT`，不尝试其他 provider。
- Minori AVI preview 与 playback 不再拥有 family codec wrapper；AstraMedia FFmpeg provider 在 demux、packet、timestamp、frame、PCM 和 cancellation 边界执行统一预算校验。Minori preview 只在调用 provider 前拒绝空输入、超限输入和非 RIFF/AVI 头，所有 provider/codec/帧越界均返回 `ASTRA_EMU_MINORI_AVI_*` 或 `ASTRA_FFMPEG_*` blocking diagnostic。
- 旧的 family-owned AVI stream table（包括 `AviStreamFormat::Other` 与 WMV3/PCM 专用分支）已在后续媒体迁移中删除；现行测试只保留容器身份、输入/帧预算和 FFmpeg binding 的 blocking 回归，不把旧 decoder 证据计入当前 codec 覆盖。
- 新增 4 个定向回归（预览输入预算、空/截断容器、尺寸/帧预算），`astra-emu-minori` AVI tests 为 4/4。该项只收紧资源边界，不扩大 codec 覆盖，也不改变真实 movie parity、完整路线或 Windows E3 的证据边界。

Linux read-only FUSE 的 EOF read 也已收紧：offset 位于文件尾或请求长度被截为零时直接返回空数据，不再把合法 EOF 误报为 `EIO`。该路径仍需真实 Linux mount/list/stat/random-read/unmount evidence，Windows 工作树不能据此标记 FUSE 完成。

### 2026-08-26 稀疏 Headless checkpoint 物化

- 发现 `--frame-sample-interval` 大于 1 时，运行路径会按契约跳过非采样 tick 的 presentation；如果 checkpoint 恰好先于首个采样 frame，直接 readback 会返回 `ASTRA_EMU_HEADLESS_CHECKPOINT_SURFACE_MISSING`。这是真实的采样与 checkpoint 契约冲突，不是输入或资源问题。
- `astra-emu-cli` 现在在捕获 checkpoint 前调用 `ensure_checkpoint_surface`：优先提交 pending retained scene，其次绘制已绑定的 GPU scene，CPU layer 则只物化当前 prepared frame；该过程不推进 fixed tick，也不改变 runtime state。没有可提交 surface 时仍 fail fast，不生成替代帧。
- 重新编译并签名当前 ABI v9 package 后，以 `frame_sample_interval=60` 运行真实八包短程：431 fixed steps、7 presented frames、27 条物理输入、无 diagnostic，报告为 `passed`；title_initial、ending 和 route_5 checkpoint 均可读，确认稀疏采样不再破坏声明的 checkpoint。该结果仍是非 terminal smoke，不替代完整四路线、正式 audio review 或 Windows E3。
### 2026-08-26 Headless await 诊断与影片链回归

- 为 Headless 的 typed await 超时与匹配日志补充有界状态计数：pending input/time/media wait、active video，以及最近一次 provider status、wait/event/blackboard 数量。日志不包含正文、资源 payload、路径或 key；计数只用于定位输入序列和媒体 fence 的边界。
- 授权样本的短路线复核确认：首段影片完成后，runtime 已发布下一脚本的 input wait；若继续保持 Control 并用 Enter 消费消息，Minori 语义会把下一条消息转换为 time wait，因此随后期待 `runtime.input_or_terminal` 会超时。这是测试输入未释放 Control 的语义问题，不是 chain、影片完成或 message wait 丢失。正式路线输入必须在需要逐条等待前释放 Control。
- 新增纯 Rust 回归覆盖“movie fence → chain → next message input wait”，并通过；直接从下一脚本入口运行的 Headless 短流程也通过，说明 chain 恢复和首个消息 wait 可独立观察。当前完整首条路线、自然解锁、正式视觉/音频审查及 Windows E3 仍未闭合。

### 2026-08-26 稀疏 checkpoint 的当前帧修正

- 复核发现：此前 `ensure_checkpoint_surface` 只在首次提交 surface 时工作，稀疏采样下后续 `config` checkpoint 可能重复写入标题帧。现已改为在每个 checkpoint 物化当前 pending retained scene、CPU layer 或 video overlay，不推进 fixed tick；没有可提交的当前 surface 仍直接阻断。
- 修正后的短 Headless 运行通过 typed await，配置页截图已显示实际 Minori 系统页，且与标题帧分离；标题、配置、影片活动帧和首条消息均已实际查看。该证据只覆盖 checkpoint 当前帧和输入语义，不能替代完整首条路线、自然解锁、正式音频审查或 Windows E3。

### 2026-08-28 Native audio 无设备时的 Null endpoint

- `astra-emu-family-support::FamilyAudioService` 在选定的 native host 打开输出时仅对 `ProviderUnavailable` 启用有界、定速的 `NullAudioLane`，仍然经过同一个 Kira mixer、重采样和 telemetry 路径，并回收所有权限缓冲区。
- native device 仍为首选；只有设备不可用时才选 Null endpoint，其他 platform error 仍然直接阻断。选择仅记录 `ASTRA_EMU_AUDIO_NULL_DEVICE` warning，不记录音频内容。
- Windows Sandbox 现在可以在无音频设备的环境中启动并进入后续 runtime 路径；Null endpoint 不构成物理音频 E3 证据，当前剩余验证阻断状态仍需单独记录。

### 2026-08-28 消息点击与 host await 合约对齐

- Sandbox 的第一轮真实输入在画面区域点击后出现两个同时 ready 的 `input` wait。诊断记录确认 family 在同一固定步只发出一个新 wait，而 host 仍保留旧 wait；根因是 Minori family 直接接受 `pointer.primary` 完成消息，但对外的 host await key 集合没有声明该输入，导致 provider 已推进脚本而 Manager 没有移除对应的 await。
- `MINORI_MESSAGE_HOST_AWAIT_CONTROLS` 现在显式包含 `enter`、`space`、`escape` 和 `pointer.primary`，与 `MINORI_MESSAGE_INPUT_CONTROLS` 的直接输入语义一致。Manager 仍只移除实际完成的 pressed edge，Escape 的系统菜单语义保持可见；没有清空 pending wait、忽略第二个 token 或添加 fallback。
- 增加了 Minori provider wait-contract 与 Manager edge-retention 回归。当前签名 Release package 在 Windows Sandbox 中完成画面点击后再确认输入，firefly Layer2D 帧继续变化且未出现 `ASTRA_EMU_AWAIT_MULTIPLE_READY`；该运行没有声明完整路线、物理音频或 Windows E3 通过。

### 2026-08-28 Control/Auto 消息等待重绑定

- Sandbox 中连续按下 Control 暴露了新的 host/provider 边界：Minori 会把当前消息的同一 family token 在 `Input` 与 `Time` modality 之间重绑定，但 Manager Core 和 host 仍按“同 token 必须新建 await”处理，第二次 Control 因而错误返回 `ASTRA_EMU_AWAIT_TOKEN_DUPLICATE`。
- Manager Core 现在为每个 family wait 保存受限的 `AwaitBinding`。只有 `Input`↔`Time` 的双向 modality 变更复用已有 `AwaitTokenId` 并更新绑定；相同 modality、其他 wait 类型或同一输出内重复 token 仍 fail fast。Manager host 同步替换 pending condition，不清空等待表、不丢弃 completion，也不推进额外 fixed tick。
- 增加 core 与 Manager 定向回归，覆盖双向重绑定及同类重复阻断。真实 Sandbox 复测需使用更新后的签名 Release package；在复测完成前，本节只记录已验证的代码路径，不把 Windows E3 或完整路线状态前移。

### 2026-08-28 右键系统菜单与活动消息等待

- Manager GameView 当时已把舞台内的物理 secondary-pointer 按下/释放事件按原坐标提交为 `pointer.x`、`pointer.y` 和 `pointer.secondary`。该版本把右键直接映射到 Save 页，后来确认与原版菜单语义不符；v11 已删除这条映射。
- 复测确认右键会显示真实 Minori `Savedata` 页面及 Auto/Quick Save 槽位。此前在该页按 Escape 会把底层 gameplay message await 一并完成，provider 随即以 `ASTRA_EMU_MINORI_SYSTEM_RESULT_UNEXPECTED` 终止；根因是 Manager 没有把系统页视为独占输入层。
- 现行 host 在右键打开请求所在 tick 以及 `minori.system_page != none` 期间暂缓 gameplay await completion，同时保留 pending wait。关闭页面只更新系统页 observation；下一次普通确认才完成原有 message wait。未知页面值、同一批重复 page mutation 和非法等待类型仍 fail fast。
- 新增 Manager 回归覆盖 secondary-pointer open、system-page activity observation 和 unknown page rejection。更新后的签名 Release 在 Windows Sandbox 中连续完成两次 Save→Escape→gameplay 循环：右键显示 Savedata 页面，Escape 返回 firefly gameplay 帧，Diagnostics 保持 `No blocking diagnostic`；Null audio endpoint 仍只提供无设备软件运行证据，不是物理音频 E3。完整路线、正式音频审查和 Windows E3 仍不提升证据等级。

### 2026-08-30 key-file Release 路线复核

- 当时分支已 rebase 到 `origin/master` 的 `a2bb57d9c43084ec8e519b8c38d5aded14f51cb3`。冲突按当时 Family API v12 解决，没有恢复旧 surface、snapshot、Luau callback、明文 cache 或兼容入口；v11 只作为历史菜单迁移记录。随后 v13 typed system-command hard cut 已记录在本文末尾。
- 官方桌面构建器此前只给 Minori library 启用 dynamic export，Manager 与 CLI 没有编译 `ffmpeg-vcpkg`，生成的包无法播放 Minori 影片。构建器现按 family 绑定 feature：Minori package 同时编译两个产品 host 的唯一 FFmpeg provider；依赖缺失会让 Release 构建直接失败。工具单元测试覆盖 Minori/FVP 的 feature 集合。
- PAZ lookup 现按原引擎文件系统语义执行 ASCII case-insensitive 匹配，同时继续向 manifest 和调用方返回 canonical URI；大小写折叠冲突阻断，不覆盖 entry。positional parser 只接受真实样本观察到的单个尾随空字段。primary `.effect fadeout` 只结束活动 Firefly，缺少目标时阻断。
- 全包 census 中只有一条三 operand `.panel`。原程序 parser 已确认字段为 mode、可选过渡和文件名；真实命令使用 `*` 作为缺省过渡并引用一个存在的 `sys` 资源。runtime 与资源预审现在共同接受这一种已验证形态；显式数值过渡、其他 mode 和非法文件名仍返回 `ASTRA_EMU_MINORI_RUNTIME_PANEL`。
- 更新后的开发签名 Release package 以序列化物理输入完成标题启动路线：`astra.emu.headless_run_report.v3` 为 `passed`，5212 fixed steps、4382 个提交/栅格帧、17 条输入、5993 次 VFS read、5113463957 bytes，diagnostic 为空。路线发布 `route_complete`，返回标题并观察到解锁计数 4。WAV 是 48 kHz 双声道、4120576 frame，peak 32412、RMS 4281.56，非静音；这不是具名人工听审。
- 模型实际检查 `title_initial`、剧情和 `ending_return_title` 三个 checkpoint。首尾标题帧一致；剧情帧的背景、面板和日文正文非空，未见缺字、横向裁剪、拉伸、错层或残影。该检查只给出当前 checkpoint 的质量结论，不声明原版逐像素一致。
- 证据边界仍有两项必须保留。第一，形成通过报告时平台私有进度已经包含四个 clear flag；此前从三个 flag 自然写入第四个的运行确实命中 `route_complete` 和解锁计数 4，但因为测试脚本在标题后错误等待 runtime terminal 而没有形成 passed report。第二，一次未跳过的 WMV3 全流解码出现 FFmpeg concealment 输出，尚未用影片 checkpoint 与原版画面对照。四路线自然解锁、save/restore required checkpoint、正式音频听审、完整 gallery、Release Sandbox 和 Windows E3 因此继续开放。

### 2026-08-30 Family 系统页输入所有权与存读档复验

- Headless 进入 Load 页后按 Enter 曾同时完成底层 message await，并把同一次输入交给 family 页面，触发 `ASTRA_EMU_MINORI_SYSTEM_RESULT_UNEXPECTED`。问题位于 Host 输入所有权，不是 Minori load transaction。Family API 现在定义公共 `astra.emu.system_ui_active` observation；Minori 只发布规范布尔值，CLI 和 Manager 在活动期间保留底层 wait。family 私有的 `minori.system_page` 只用于页面观察，不再承担 Host 控制语义。
- 更新后的官方开发签名 Release 候选以序列化物理输入完成 Quick Save、推进到下一消息、打开 Load 页并恢复 slot。报告为 `passed`：2047 fixed steps、1685 个提交/栅格帧、6 个 checkpoint、零 diagnostic；WAV 为 48 kHz 双声道、1617920 frame，自动量测非静音且无 clipping。Load 页确实使用原版系统资源，恢复 checkpoint 与保存前 checkpoint 字节一致。这是当前身份的定向 Headless E2，不是 Release Sandbox 或正式 Windows E3。
- 模型查看保存前、推进后、Load 页和恢复帧时发现一组只在单条真实 message 行末出现的控制标记被显示成普通字符。全包私有统计确认该组合唯一，不能据一次样本猜测为可直接删除的装饰符。原版在普通消息末尾使用独立推进指示，但这组标记的组合效果仍需原程序观察或反编译确认；在修复和 checkpoint 复验前，消息视觉完整性继续 blocking。

### 2026-08-31 Family ABI v13 系统命令与平台 Host

- Family ABI 从 v12 硬切到 `astra.emu.family_abi.v13`。确认框和菜单层级仍由
  family 发布；菜单选择的窗口、帮助和关于动作不再生成通用 `LegacyEvent`，而是
  发送有界 `LegacySystemCommandTransactionV1`，结果在下一固定 step 以
  `LegacySystemCommandResultV1` 回传。旧 v7–v12 fingerprint 在加载前拒绝，未保留
  compatibility shim。
- Minori 当前已验证 `window_fullscreen` 选择会发布 `SetFullscreen`，只有收到 Host
  的 `Applied` 才更新 runtime config。`RestoreOriginalSize`、缩放精度/抗锯齿以及
  Help/About/Homepage 均保留 typed kind；尚未确认或未绑定的能力返回 `Unsupported`
  并阻断当前命令，不猜测 URL、窗口句柄或系统 UI 语义。
- Windows Host 通过 live window 的原生 fullscreen 与记录的初始 client size 完成
  两项窗口动作；macOS 在 event-loop 主线程使用同一平台 port，并把 winit flipped
  view 的坐标转换留在 Host。Manager、Headless 和无窗口 CLI 不解释命令，只回送
  `Unsupported`，避免把 Slint overlay 或虚拟输入当成平台证据。Linux 当前缺 GTK
  window binding，保持显式 unsupported diagnostic。
- 新增 Family API FFI wire round-trip、Manager completion matching、Minori applied
  result 和 Windows/macOS platform command checks。当前定向 checks 通过；Linux GTK
  原生菜单、Help/About 行为、完整路线、Release Sandbox 和 Windows E3 仍未关闭。

### 2026-08-31 Host-native system dialogs and resize policy

- 原版 Sandbox 观察确认：Game→Exit 与 Game→Return title 使用带 `是(Y)`/`否(N)` 的
  原生确认框；Help→Help 启动 HTML Help，Help→About 显示版本和版权信息，Help→
  minori homepage 请求系统浏览器。Family 只报告有界的 typed command，正文和外部
  进程状态不写入 VM、save、replay 或 evidence。
- `astra-platform` 现提供 `OpenManual`、`ShowAbout`、`OpenHomepage` 的 Host 命令和
  bounded request DTO。Windows Host 绑定 `hh.exe`、Win32 ShellExecute 与 `rfd`，
  macOS 绑定系统 `open` 与原生 About 对话框；Linux、Web、Android、Headless 没有
  原生窗口能力时返回稳定 unsupported diagnostic。手册路径只在 Host 边界作为已验证
  的本地文件使用，不进入 Family ABI。
- `SetResizeAntialias` 现在由 Windows/macOS Host 转换为对应 presentation surface
  的 GPU sampler 切换。`astra-platform-common` 保留线性和最近邻 sampler，在命令边界
  切换，不复制或重建 Minori scene；`SetResizePrecision` 仍是 Host-owned typed
  operation，原版菜单当前为禁用且勾选状态，未凭名称扩展未知语义。
- Minori 收到 Host `Applied` 后只释放挂起的 system-command transaction。窗口尺寸、
  sampler、帮助对话框和浏览器属于 Host 外部状态，不伪造为确定性 VM state；`Rejected`
  与 `Unsupported` 仍阻断 session。六种 host-owned command 已有 provider 回归，
  platform DTO validation、公共 presentation core 与 Windows target check 通过。
- About 对话框的原版图像/排版、Linux GTK 原生菜单、帮助和浏览器实际启动结果尚未
  形成正式 Windows E3 或 Release Sandbox evidence；这些差距保留为下一轮验收项。

### 2026-08-31 macOS 原生确认框标题绑定

- macOS 的确认框现在优先读取当前宿主窗口标题，并继续使用 `rfd` 的原生 Cocoa
  对话框；只有宿主没有标题时才使用 Family ABI 携带的请求标题。这样退出和返回标题
  的确认框与 Windows 的 owner-caption 语义保持一致，Family 不需要知道平台窗口句柄。
- 该改动只改变 Host presentation，不改变确认 transaction、输入消费或 VM 状态。当前
  Windows 定向测试通过；macOS 交叉检查受限于本机没有 Apple C/链接工具链，尚未形成
  macOS 运行时 evidence。

### 2026-08-31 Linux 原生确认框标题绑定

- Linux Host 的确认框也在原生 `rfd` presenter 边界读取 live owner caption；没有宿主窗口的
  Manager/service 调用继续使用 Family transaction 的标题。这样各桌面 Host 对退出和返回标题
  的 caption 选择保持一致，同时不把平台窗口句柄或标题判断下沉到 Minori。
- Linux 的 context menu、窗口命令和 Help/About/Homepage 仍按能力矩阵返回显式
  `ASTRA_EMU_PLATFORM_CONTEXT_MENU_UNSUPPORTED` 或对应 unsupported diagnostic；没有用
  临时 GTK widget、Slint overlay 或外部进程伪造原生菜单证据。Linux 编译和实际桌面行为仍需在
  具备 GTK/winit 原生窗口的环境中单独验收。

### 2026-08-31 Family ABI v14 文本输入与保存注释

- Sandbox 观察确认 Save 空槽不是立即写入，而是先出现 Host-owned 的单行 Comment
  编辑框；Family 只发布 prompt id、标题、标签、初值、按钮和 UTF-8 字节上限，Host
  负责焦点、IME、Enter/Escape、DPI 和 owner-modal 生命周期。正文不进入日志、report、
  replay 或路径相关字段。
- Family API/FFI v14 增加 `LegacyTextInputTransactionV1` 和 typed result。Manager
  只维护一个 session 内的 pending/resolution 队列；CLI Headless 通过显式的 typed
  输入事件完成 Accepted/Cancelled，不能把字符事件伪装成 gameplay input。Minori
  只有 Accepted 才把注释写入与 slot payload 同一 writable-file 原子替换，Cancelled
  保持底层 wait 和存档文件不变。
- Windows 已接入 owner-modal Win32 单行编辑框，按 live owner/system DPI 建立布局并
  恢复焦点；macOS、Linux、Web、Android、Headless 当前明确返回
  `PlatformNotImplemented`，待各自宿主绑定原生 UI 或 typed driver，不回退到 Slint、
  Manager overlay 或逐字节模拟。Save 页已保存的注释如何在列表中显示尚未由原版同点
  观察和资源证据闭合，因此保持为下一验收项。
- `astra-emu-family-api` 38/38、Manager Core 文本输入/系统事务 5/5、Minori
  180/180、`astra-platform` 11/11 和 Windows target 9/9 定向测试通过；这些是
  contract/provider/Host 接线证据，不是完整路线、Release Sandbox、正式音频或 Windows
  E3 证据。

### 2026-08-31 Save slot comment rehydration

- 在不改变原版空槽页面资源布局的前提下，Minori 现在会在 Save/Load 页面首次刷新
  以及槽文件长度变化后读取并校验 v2 slot envelope，只保留有界的用户注释供下一次
  Host-owned Comment prompt 使用。重复刷新不会逐 tick 重新读取未变化的槽文件。
- 槽文件的 schema、case/package/profile identity、长度和注释字符边界任一不满足时，
  页面刷新返回稳定的 `ASTRA_EMU_MINORI_SAVE_LIST_*` diagnostic；不会把损坏内容
  当作空槽，也不会把槽正文写入报告或日志。Save 页面中已保存注释的具体绘制位置
  仍需原版同点资源/行为证据，当前不做猜测。

## 2026-08-31：Native menu anchor follows the rendered stage

原生 CLI 之前把 Family 发布的菜单锚点丢弃，依赖当前系统鼠标位置。这样在回放
物理输入、窗口缩放或画面留边时，菜单可能出现在错误的位置。现在 CLI 在调用
`astra-platform` 前把舞台坐标映射到 live window 的物理 client 像素；Windows
直接交给 `muda-win`，macOS 再按窗口 scale factor 转成 AppKit logical view
坐标并翻转 Y。锚点越界、视口无效和整数溢出都会阻断，不会回到系统光标。

新增的 runner 回归覆盖 16:9 留边、舞台边界和负坐标。该项只证明 Family ABI
语义到 Host 的坐标传递，尚未形成 Linux 原生菜单、Release Sandbox 或 Windows
E3 证据。

同日补充 Host 侧层级校验：Windows/macOS 在构造 OS 菜单前重新检查 parent
存在性、parent kind、sibling order、循环和四层深度上限。这样即使未来出现不合格
的 Family transaction，也会在 `window.context_menu` 边界返回稳定 diagnostic，
不会让平台菜单 API 接收断裂树或由 Host 静默丢弃分支。该校验是契约防线，不改变
Minori 菜单的 item id、顺序或平台呈现。

## 2026-08-31：当前 Release Headless 输入契约 smoke

- 在已通过启动检查的开发签名 Release 候选上，使用显式 WMF provider 和当前
  `astra.user_input_sequence.v1` 输入，完成 33 条物理输入、4008 个 fixed step、
  491 个提交/栅格帧和 2713600 个音频帧；`astra.headless_run_report.v2` 为
  `passed`，diagnostic 为空，输入序列完整消费。
- 人工查看了首个黑场、首条日文消息、背景场景和人物消息四个代表性 checkpoint。
  舞台比例、消息框位置、日文字形、背景层次和推进指示均可见，未发现裁剪、拉伸、
  缺字或错层。该检查只给出本次短程输入的质量结论，不是原版同点像素 parity。
- 这次运行只证明当前 Family ABI 菜单/确认接线、WMF 绑定和新输入观察键在 Release
  CLI Headless 中可以稳定完成一个有限片段；四路线完整运行、save/restore required
  checkpoint、正式音频听审、Release Sandbox 视觉验收和 Windows E3 仍保持开放。

## 2026-08-31：48 checkpoint Release Headless slice

- 使用同一开发签名 Release 候选、显式 WMF provider 和校正过 checkpoint 顺序的物理
  输入序列，完成 193 条输入消息、24048 个 fixed step、238 个提交/栅格帧和
  16757248 个音频帧；48 个 checkpoint 全部通过，报告为 `passed` 且 diagnostic 为空。
  该序列在尾部显式 shutdown，没有把有限片段误报为 terminal。
- 查看了早段黑场、中段日文消息、背景过渡和后段人物构图。消息框、日文 glyph、背景
  层次、转场颜色与人物裁剪均可见，未发现明显拉伸、缺字、残影或坐标漂移；图片只
  留在 ignored 私有 artifact，不进入仓库或公开报告。
- 这是当前输入契约下的长片段 Headless E2 质量证据，不能替代四路线自然结局、
  save/restore required checkpoint、原版同点视觉、正式音频听审、Release Sandbox
  或 Windows E3。此前将 checkpoint 放在同 tick await 之后的旧序列会被
  `ASTRA_EMU_HEADLESS_CHECKPOINT_ORDER` 拒绝，现已修正输入生成约束。

## 2026-08-31：只运行日文原版与严格 CP932 locale hook

- `astra.emu.minori.mount_options.v3` 现在要求 `content_variant` 为
  `natsuzora-no-perseus.original-ja`、`locale_hook` 为
  `astra.emu.minori.locale.ja-jp.cp932.v1`，并在挂载时确认 `perseus.exe` 是原版的
  非符号链接普通文件。该边界来自当前游戏目录的原版/本地化文件并存观察，不把本地
  路径或商业文件内容写入公开记录。
- `perseus_chs.mys`、本地化 exe 和汉化备份目录只作为 inventory 事实；本轮没有
  汉化版入口、MYS overlay 或翻译资源解析。locale hook 是 host 可绑定的转区/编码
  语义，严格执行 CP932 decode/encode，用来避免非日文宿主的乱码，不会替换正文，也
  不会回退 GBK 或 translation provider。
- PAZ index/entry key、ANI frame name 和 `.sc` lossless parser 已统一经过该 binding；
  malformed CP932、未知 hook 或本地化变体均以稳定 diagnostic 阻断。Minori message
  publisher 删除了 production translation Hook 调用，原版日文正文直接进入
  CosmicText/Renderer2D 路径。
- 新增 locale round-trip、非法 CP932、变体/入口拒绝和严格 ANI 名称回归；
  `astra-emu-minori --lib --no-default-features` 定向 189/189 通过。证据等级为
  E1/E2 的代码与定向测试边界；不关闭四路线、原版同点视觉、Release Sandbox、正式
  音频审查或 Windows E3。

## 2026-09-01：日文原版脚本 census 复核

- 使用当前 `mount_options.v3` 日文原版 profile 重新读取授权样本的脚本树，严格
  CP932 解码后得到 89 个 `.sc`、33728 行和 33695 条 command；29 个已观察 opcode
  均在 census 中出现，unknown opcode 为 0。结构计数包含 55 个 `chain`、20 个
  `if`、10 个 `goto`、2 个 `select`、15 个 `movie` 和 85 个 `end`，与已提交的
  脱敏脚本基线一致。
- 两条 `select` 均保持原始 `display:label` 形式，四项与三项选择各一条；解析器只
  记录选项 hash 和 label，不把正文写入 report。`.include` 目标现在也经过同一
  日文 locale binding，再进入安全的 ASCII `.sc` URI 校验，非法 CP932 直接阻断。
- 本轮只新增 census 和 parser 边界证据，没有执行汉化入口、MYS overlay 或翻译
  provider。该项属于 E1/E2 解析复核，不关闭四路线运行、原版同点视觉、Release
  Sandbox、正式音频审查或 Windows E3。

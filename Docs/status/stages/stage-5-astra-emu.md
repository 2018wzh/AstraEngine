# Stage 5 AstraEMU Work

2026 年 8 月 31 日 Minori 原版内容边界已收紧：launch profile 必须选择
`natsuzora-no-perseus.original-ja`，factory 只挂载带有非符号链接 `perseus.exe` 的
日文原版目录；汉化 exe、`.mys` 和其它本地化覆盖不会进入 runtime。PAZ、ANI 和脚本
文本统一使用严格 `astra.emu.minori.locale.ja-jp.cp932.v1` 转区绑定，非法字节直接
阻断，不做翻译或 GBK 回退。Minori message publisher 已删除 translation Hook；定向
library 202/202 通过。该项只关闭当前实现的原版/乱码边界，Stage 5、完整路线、视觉
音频审查、Release Sandbox 和 Windows E3 仍为 `IN_PROGRESS`。

2026 年 9 月 1 日消息推进指示符布局复核：原版短消息中的下三角与正文同一行；Minori
文本 surface 改为正文/指示符独立 CosmicText layout，并按最后一行测量宽度定位，
避免第二个 `TextRun` 导致的意外换行。当前 v4 Release Headless 短程完成 4008 fixed
step、8 个 checkpoint、491 个提交/栅格帧和 2714112 个音频帧，diagnostic 为空，
消息 checkpoint 已人工查看。该项只计单点视觉 E2 修正，Stage 5、同点 parity、完整路线、
正式音频、Release Sandbox 和 Windows E3 继续保持 `IN_PROGRESS`。

2026 年 9 月 1 日行末 message markup 恢复边界：私有 census 确认唯一的 `\\v\\a` 组合
属于真实路线消息的控制序列；原版首段 Sandbox 只观察到独立下三角，尚未在无音频输出的
环境中可靠推进到目标语音等待点。Minori snapshot/backlog 现在严格重解析持久化可见文本，
发现残留控制序列即返回 `ASTRA_EMU_MINORI_MESSAGE_CONTROL_NONCANONICAL`，不静默剥离。
该项只收紧损坏状态的 fail-fast 边界，不关闭目标行同点视觉、四路线、正式音频、Release
Sandbox 或 Windows E3，Stage 5 继续保持 `IN_PROGRESS`。

2026 年 9 月 1 日标题页 pointer 对齐：Minori 按 stage-space 处理原版右侧菜单 hover 与
点击，使用 `topMenu*Over.png` 行裁剪并复用键盘 typed action；基础/解锁变体和 restore
清理均有定向回归，当前 library 202/202。该项只计 E1/E2 provider 证据，原版同点视觉、
完整路线、Release Sandbox 和 Windows E3 继续阻断 Stage 5 完成。

2026 年 8 月 31 日标题页 Exit 回归已加入 provider 测试：标题页直接结束 session，不发布
confirmation；剧情页退出仍经 Family ABI 交给 Host 原生处理。该项只覆盖已观察语义的
provider/Host 边界，Stage 5 继续为 `IN_PROGRESS`，Release Sandbox、完整路线、视觉/音频和
Windows E3 仍未闭合。

2026 年 8 月 31 日剧情确认框文案对齐：授权 Windows Sandbox 复核确认 Game→Exit 与
Game→Return title 的正文采用 ASCII `?`，标题页 Exit 直接结束程序。Minori Family ABI
继续发布有界 confirmation transaction，Host 只呈现原生 owner-modal 对话框并回送 typed
结果；定向 provider/Host 测试通过。该项不关闭完整路线、Release Sandbox、视觉/音频
审查或 Windows E3，Stage 5 仍为 `IN_PROGRESS`。

2026 年 8 月 31 日原版菜单禁用项导航对齐：授权 Windows Sandbox 的标题右键菜单
确认方向键焦点会经过灰色禁用项、跳过分隔线；鼠标点击禁用项不提交命令，Enter
关闭弹出菜单而不选择，Space/Right 保持菜单活动；submenu 返回时焦点恢复到父项。
CLI Headless 与 Manager Host 按 Family ABI `order` 保留这些焦点项并区分三种结果，
定向测试通过。该项只修复 Host 导航语义，不提升真实 Manager 窗口、完整路线、
Release Sandbox 视觉验收或 Windows E3，Stage 5 仍为 `IN_PROGRESS`。

2026 年 8 月 31 日 Family ABI v14 文本输入 Host 分层：Minori Save 空槽按原版行为
发布有界的 `LegacyTextInputTransactionV1`，由 Host 原生编辑器负责焦点、IME、DPI、
Enter/Escape 和按钮语义，结果在下一固定 step 以 typed value 回送；Accepted 才以
writable-file 原子写入保存注释，Cancelled 不消费剧情 wait。Windows 已接入 owner-modal
Win32 单行编辑框；macOS、Linux、Web、Android、Headless 当前明确返回
`PlatformNotImplemented`，要求各自绑定原生应用 UI 或 typed driver，不回退到 Slint overlay。
Family API/FFI、Manager queue、Minori save envelope、CLI virtual input 和 Windows target
定向回归通过。该项只关闭文本输入事务的 Host 接线，不构成 Save 页注释视觉 parity、完整
路线、Release Sandbox、正式音频 review 或 Windows E3 证据。

2026 年 8 月 31 日 Windows confirmation geometry 对齐：Host 原生 presenter 采用
原版观察到的 350×164（96 DPI）紧凑基准，按 live owner window 的有效 DPI 缩放控件（无
owner 的 service 调用使用系统 DPI），
并在消息左侧放置标准 Win32 question icon。checked arithmetic、`是(Y)`/`否(N)`
助记键、owner-modal 生命周期和 Family ABI v14 transaction 不变；该项只增加平台
呈现回归，不构成原版同点视觉、Release Sandbox 或 Windows E3 证据。

2026 年 8 月 31 日 Release Sandbox 启动复核：首个开发包因机器 VC runtime
依赖无法启动；官方 desktop builder 已改为 Windows MSVC 强制 `+crt-static`，
并在环境显式要求动态 CRT 时阻断。新的开发签名 Release 包在同一 Sandbox
成功打开 AstraEMU Manager，PE import 检查没有发现 `MSVCP140` 或 `VCRUNTIME140`
依赖。该证据只关闭分发包启动阻断，不提升完整 Minori 路线、Headless E2、正式
视觉/音频审查或 Windows E3；Stage 5 继续为 `IN_PROGRESS`。

2026 年 8 月 31 日 Linux native confirmation caption 对齐：Linux Host 的
`rfd` presenter 在存在 live owner window 时沿用 owner caption，无 owner 时使用
Family ABI transaction title。该改动保持 context menu、窗口命令和 Help/About 的
unsupported 能力边界，不引入临时 GTK widget 或平台 fallback；Linux 实际桌面
验收与 Stage 5 总体状态仍为 `IN_PROGRESS`。

2026 年 8 月 31 日 Windows Manager confirmation service 接线：无游戏窗口的
audio/decode service host 现在直接在 service thread 处理 Family ABI v14
`ShowConfirmation`，使用 Host-owned Win32 modal window，并把 typed 结果送回同一
session。移除 `rfd` `common-controls-v6` 后，CLI 不再在进入 `main` 前触发
`STATUS_ENTRYPOINT_NOT_FOUND`；Win32 对话框保留 Minori 自定义按钮标签。该路径不
创建第二个 Winit loop，也不处理菜单、全屏、帮助、About 或 surface；这些仍要求
拥有窗口的对应 platform Host。`astra-platform-windows` confirmation 回归 2/2、
CLI 启动检查、Manager build 和文档检查通过，但这只是 Host 事务接线证据，Stage 5、
Release Sandbox、正式音频 review 和 Windows E3 仍为 `IN_PROGRESS`。

2026 年 8 月 31 日 Family ABI v14 原生菜单 Host 分层：Windows 原生 context menu 和
confirmation 保持不变，macOS 通过 AppKit `muda` 接入同一 typed menu transaction，
并在主线程处理 flipped view 坐标；CLI 的 macOS native 路径已接通。窗口/帮助动作
也通过 Host port 处理：Windows 使用 `hh.exe`、ShellExecute 和 `rfd`，macOS 使用
系统 `open` 与原生 About 对话框。窗口抗锯齿切换由 WGPU presentation core 选择
linear/nearest sampler，不重建 family scene。Linux、Web、Android、Headless 对没有
原生窗口能力的动作返回稳定 unsupported diagnostic，不保留悬挂菜单或隐式 fallback。
该项是平台 Host 接线证据，About 图像、完整路线、正式视觉/音频、Release Sandbox
和 Windows E3 仍为 blocking。

2026 年 8 月 31 日剧情右键菜单的两个顶层分类已按 Sandbox 观察改为日文标题
`ヘルプ (&H)`、`ゲーム (&G)`，移除英文值并加入 family menu regression。菜单
语义仍由 Minori Family ABI 发布，Windows Host 只构建/呈现并回送选择；完整 Release
Sandbox、Linux 原生菜单和 Windows E3 继续保持 blocking，macOS 原生菜单已接通。

同轮 Manager 的 Host UI 完成层级菜单导航：只显示当前父节点的启用同级项，方向键
循环移动，Right/Enter/Space 进入 submenu 或提交 command，Left/Escape 返回或关闭。
焦点事件在菜单激活期间不会送入 gameplay，适配器与 Manager Host 都会重验 active
transaction、parent、kind 和 enabled 状态。该项只有定向 UI/Host 回归，真实窗口焦点、
锚点和原版同点截图仍未形成，因此 Stage 5 继续为 `IN_PROGRESS`。

同轮把菜单选择的窗口/帮助动作改为 `LegacySystemCommandTransactionV1`。Family 只
发布 typed kind，Host 按当前平台窗口绑定执行全屏、原始尺寸恢复、缩放采样和外部
帮助动作，再以 `LegacySystemCommandResultV1` 回送结果。Minori 对 Host 已 `Applied`
的 host-owned 操作只解除挂起事务，不把窗口、浏览器或帮助进程写入 VM 状态；Host
拒绝或不支持仍由 Family 阻断。Manager、Headless 和无窗口 CLI 对未绑定能力显式
回送 `Unsupported`。

2026 年 8 月 31 日 Windows native confirmation 标题已按 Sandbox 观察对齐：有 parent
window 时沿用 live game caption，没有 parent 时保留 Family ABI 的 typed title。该
行为由 platform provider 实现，不让 Manager 或 Minori 复制商业标题；Windows target
check 与平台确认回归通过。仍只有局部行为证据，完整 Release Sandbox、音频 review 和
Windows E3 未关闭。

2026 年 8 月 31 日确认框输入对齐：Headless Host 在 confirmation transaction 存在时
消费原版 `是(Y)`/`否(N)` 快捷键，并把它们转换为 Family ABI 的 typed choice；确认框
关闭后同样的 Y/N 输入仍然被拒绝，避免把系统快捷键泄漏到剧情。Windows 原生对话框
继续交给平台消息框处理。该项只有定向输入回归证据，不提升 Release Sandbox、完整
路线、正式音频听审或 Windows E3 状态。

2026 年 8 月 30 日原生菜单 Auto E2：受限 wait 重绑规则已收敛到 Manager Core，RuntimeWorld adapter、Manager 和 Release CLI Headless 共同只允许 `Input↔Time` 和 `Time→Time`，其他重复 token 继续阻断。开发签名 Release v21 从空白进度通过 family v11 菜单选择 Auto，再次选择后恢复 Normal。报告通过 371 fixed steps、36 条物理输入、9 个呈现帧、6 个 checkpoint且零 diagnostic；Auto 开启后场景按预期推进，关闭后继续运行 2 秒画面不变。关键画面已人工检查，Manager Core 59 项 tests、CLI 定向回归和三个 consumer 的 clippy 通过。该项关闭原生菜单 Auto 定向 E2，不替代完整路线、Release Sandbox、120 Hz 性能门禁或 Windows E3，Stage 5 保持 `IN_PROGRESS`。

2026 年 8 月 30 日 Family ABI v12 确认事务（历史记录）：Minori 的 `game_exit`、`game_return_title` 和 Host `window.close` 都通过 Family ABI 发布有界 confirmation transaction；Host 读取 transaction 后调用平台 native confirmation provider，接受或取消结果在后续固定 step 回传，family 再决定 terminal 或返回标题。Manager 不再维护第二套对话框语义，Headless 只使用物理方向键、确认键和取消键。Family API、平台、Manager、CLI 与 Minori 的定向回归通过；这属于当时 ABI 的 E1/E2 集成证据，当前 v14、Release Sandbox、120 Hz 性能门禁和 Windows E3 仍开放。

2026 年 8 月 30 日原生菜单 Skip 与 Control E2：开发签名 Release CLI 用序列化 secondary-pointer 打开 family v11 菜单，再以物理方向键选择 Skip。定向报告通过 674 fixed steps、41 条输入和 4 个 checkpoint，diagnostic 为空；未读消息在选择前、选择后和 10 秒后的画面文件完全一致。Control 快进现在由启用 pragma 的 Host-owned message wait 接收，不再用同一 token 重绑 wait；Minori 173 项 library tests 通过。独立空白进度的 Control 首路线随后通过 15636 fixed steps、28 条物理输入、251 个呈现帧和 3 个 checkpoint，结局影片自然完成，路线返回标题并退出，自然解锁数为 1，diagnostic 为空；三个 checkpoint 的人工检查未见阻断。当前结果仍不是 Release Sandbox、120 Hz 性能门禁或 Windows E3，Stage 5 保持 `IN_PROGRESS`。

2026 年 8 月 30 日 Skip 现场纠偏：原版空白进度选择 Skip 后不会推进未读消息，重新打开原生菜单时 Auto/Skip 也没有勾选。Minori 现只让持久 Skip 快进已有 read identity 的消息；受 pragma 门控的 Control 仍是独立快进路径。transaction 不再伪造 Auto/Skip checked state。Minori 173 项 library tests、受影响 clippy 与文档检查通过；该项只关闭行为级 E1 差异，Stage 5 仍为 `IN_PROGRESS`。

2026 年 8 月 30 日 WMF composition：CLI 与 Manager 现按用户/构建 profile 显式绑定 `wmf` 或 `ffmpeg-vcpkg`；Minori 不接受缺失 binding，也不会在 decode error 后切换 provider。Windows 桌面包默认 WMF，构建 evidence 保存选择。Headless 已删除写死的 FFmpeg worker，改用 `IncrementalMediaDecoder`，公开 MP4 分别通过 WMF 默认图与 FFmpeg feature 图的逐帧、EOS 回归。开发签名 Release CLI 的真实影片 slice完成 17,371 fixed steps、32 个提交帧和零 diagnostic；影片在第 5,251 tick 打开，于第 16,387 tick 完成，完整音频非静音且未削波。早段和中段 checkpoint 未见上下颠倒、拉伸或边缘裁切。该证据只关闭 WMF 定向 movie E2；原版同时间点比较、完整路线复跑、Release Sandbox 和 Windows E3 仍待执行，Stage 5 保持 `IN_PROGRESS`。

2026 年 8 月 30 日原版影片契约复核：PE/COM 调用确认原版使用 DirectShow Filter Graph、`IGraphBuilder`/`IFilterGraph2` 和 windowless VMR7；关键 HRESULT 失败进入清理分支，没有观察到后端 fallback。AstraMedia 已加入有界只读 `IStream` adapter 和显式 `astra.decode.wmf.incremental` provider，公开 MP4 覆盖统一双轨 packet、seek/cancel，授权 AVI 通过同一 provider 完成 2106 个视频 packet 和 2110 个音频 packet。固定 FFmpeg 路径继续对 7 个 concealment frame 阻断。Minori composition 与 launch binding仍开放；当前结果不关闭 movie checkpoint、原版 parity、Sandbox 或 Windows E3，Stage 5 保持 `IN_PROGRESS`。

2026 年 8 月 30 日自然鉴赏复核：同一隔离 writable identity 在四条路线自然写入后，通过物理输入进入 `Memories`、BGM、CG、回想和影片列表。Headless 报告通过 82 fixed steps、20 个呈现帧、9 个 checkpoint、45 个资源且零 diagnostic；九张保留画面已检查，未见明显缺字、裁剪、拉伸、错层或残影。影片列表尚未实际播放，原版 Sandbox 又被原程序重复异常对话框阻断，因此原版同点比较、Movie playback、四份独立绿色路线报告和 Windows E3 仍开放。

2026 年 8 月 30 日 FFmpeg 复核：Windows native dependency 现由根 vcpkg manifest 固定为 FFmpeg `8.1.2#3`，完整与增量 provider 都拒绝非 `libavcodec 62.28.102` runtime。授权 WMV3 在普通文件输入、外部 FFmpeg 和 custom AVIO 中均暴露相同 7 个损坏帧；因此 custom AVIO 不是唯一根因，固定版本也不能被写成影片质量已关闭。AstraMedia 现读取 `AVFrame::decode_error_flags` 并以 `ASTRA_FFMPEG_CORRUPT_FRAME` 阻断。required movie checkpoint、原版同点比较与 Windows E3 仍开放。

2026 年 8 月 30 日自然解锁复核：独立 launch profile 和 writable identity 从零顺序执行四条真实路线，不改动用户存档。Sui、Ayame、Tohka 报告通过，累计解锁分别严格观察为 1、3、4；Ren 已发布 `route_complete` 并完成结局影片，但私有输入的过时累计值断言使该报告按协议失败，Ayame 的累计值 3 证明 Ren 的原子写入已由下一 session 装载。相同 identity 的新标题 session 自然显示并可进入 `Memories`；两个 checkpoint 已完成模型检查。自然 clear 写入与标题 gate 已形成真实 E2 链路，但四份独立路线报告尚未全部绿色，鉴赏子页、正式音频 review、Release Sandbox 和 Windows E3 仍未关闭。未跳过的 WMV3 已到达 owner-side completion，但 FFmpeg concealment 仍是逐帧质量 blocker，Stage 5 保持 `IN_PROGRESS`。

2026 年 8 月 30 日 key-file Release 复核：分支已 rebase 到当前 `origin/master`。官方桌面构建器会为 Minori Manager/CLI 强制编译唯一的 `ffmpeg-vcpkg` provider；缺依赖时构建失败。补齐样本确认的 PAZ 大小写、positional、Firefly fadeout 与三参数 panel 语义后，开发签名 Release 包完成一条 5212 fixed step、4382 presented frame、17 条物理输入、非静音音频和零 diagnostic 的标题启动路线，发布 `route_complete` 并返回标题。三个 checkpoint 已通过模型质量检查。该 passed report 载入时已有四个 clear flag，不能替代从三个 flag 自然写入第四个的同报告；WMV3 concealment、save/restore required checkpoint、正式音频 review、Release Sandbox 和 Windows E3 仍未关闭，Stage 5 保持 `IN_PROGRESS`。

2026-08-28 no-device validation: the signed Manager opened its runtime-active window in the authorized Windows Sandbox despite the unavailable default output device; the Diagnostics panel showed no blocking diagnostic and now exposes `audio_endpoint=null` for the selected bounded sink. This confirms the NullAudioLane startup path and not physical audio, formal review or Windows E3, because the sink is intentionally excluded from evidence and the Sandbox had no writable artifact channel.

2026-08-28 audio lifecycle hardening: a failure to start the native wake relay after endpoint selection now closes and joins the already-running audio worker before startup returns. The cleanup uses the normal host shutdown contract and keeps the no-device path fail-fast; it does not change the physical-audio or Windows E3 evidence boundary.

2026-08-28 audio startup hardening: `FamilyAudioService::start_with_client` now waits for the worker's endpoint-selection handshake before exposing the service. A `ProviderUnavailable` result therefore selects and reports the bounded null lane before Minori can submit commands, while any other output error is returned synchronously and the owned native host is cleaned up. This closes the asynchronous startup race without changing the physical-audio or Windows E3 evidence boundary.

2026-08-28 follow-up: 在无音频设备的 Sandbox 中，`ProviderUnavailable` 现在由 `FamilyAudioService` 选择 bounded `NullAudioLane`，继续走 Kira、重采样和 telemetry；非该错误仍直接阻断。Manager 同时报告 `audio_null_device`，不把 null sink 的非静音 meter 当成物理音频证据。另修正 Minori 消息 wait 的 host key 集合，加入 `pointer.primary`，避免 family 已推进而 Manager 保留旧 wait。定向 Minori/Manager 测试及点击后确认的 Sandbox 运行通过，未再出现重复 ready 诊断；该证据不关闭物理音频、完整路线或 Windows E3。
同日的 NullAudioLane 边界复核还固定了输出声道、完整 mixer chunk 和有限浮点样本；错误长度或非有限值在 sink 计数更新前返回 typed diagnostic，避免无设备路径吞掉格式错误。
公开 `FamilyAudioService` 生命周期另有回归：`OpenAudioOutput` 明确返回 `ProviderUnavailable` 后，worker 仍能完成初始化、处理已排队的 suspend 请求并正常关闭，且 `null_device` 保持可观测。该测试只关闭无设备启动/关闭回归，不改变物理音频、正式音频 review 或 Windows E3 的 blocking 状态。
Support 还把 `has_physical_audible_output()` 作为 evidence 专用判定，和包含 null sink 的 mixer-level audible telemetry 分离；Manager 已改用该 API，避免后续调用方重复组合两个标志时漏掉无设备边界。

2026 年 8 月 28 日运行时复核：授权样本的短路线重跑在显式 FFmpeg provider、八包 mount 和序列化物理输入下完成 15,132 个 fixed step、5,523 个提交/栅格帧和 7,800,320 个音频帧，三个 checkpoint 与零 diagnostic 通过。另一条自然路线暴露了一个真实的时序边界：render clock 先取出的 screen-shake frame 可能在同一 tick 被 `.transition` 替换；provider 现会丢弃已失效的 frame，并以回归测试保证不把已退休的 presentation state 送入 renderer。该修复和本次 Headless 结果仍只属于局部 E2，四路线自然解锁、正式音频听审、Windows E3 与完整鉴赏继续保持 blocking。

同日的 global-progress 回归用两个独立 provider/session 覆盖 writable-file 的真实装载顺序：第一 session 执行 `REN_CLEAR` 后原子持久化，第二 session 在 `.if REN_CLEAR ...` 之前完成读取，并保持 `SUI_CLEAR` 未设置。该项只增加 provider/VFS E1 证据，不把局部状态持久化提升为四路线自然解锁或完整鉴赏通过。

2026 年 8 月 30 日（v11 历史记录）：Family API 曾进入 v11 hard cut（`astra.emu.family_abi.v11`）。右键只提交 `Open`；Minori 在稳定 title 或 gameplay 状态发布有界菜单 transaction，Host 回送 `Select`/`Dismiss` 后才执行 family 命令。该记录保留当时的菜单 E1 证据；当前身份已升级到 v14，并在同一 Family ABI 增加 confirmation 与 text-input transaction。Headless GPU E2、Release Sandbox 视觉复测和 Windows E3 仍未关闭。

2026 年 8 月 27 日媒体复核：`astra-media::IncrementalMediaPlayback` 已统一校验播放配置、单调 tick、轨道/packet 形状、音频/视频 packet 预算、视频 lead/lag 与迟到策略；Minori 继续只绑定显式 `ffmpeg-vcpkg`，不保留手写 AVI/WMV decoder 或平台 fallback。当前签名 release plugin 的标题→配置→影片→Control 跳过→标题 Headless slice 报告 `passed`，完成 3102 fixed steps、9 个 retained frame sample，诊断为空。按当前 v9 typed observation 重新生成的首路线也已报告 `passed`：3,034,309 fixed steps、16,150 条物理输入、53 个 retained frame sample、31 个 checkpoint、route terminal、自然 unlock count=1 和最终 Exit 均成立。该输入未把已删除的首 choice 等待点计入本次 checkpoint，choice 仍由独立真实 slice 覆盖。正式音频听审、四条路线后的完整 Memories/CG/BGM/回想、cache second-run、Linux FUSE、macOS extract、Manager 实机预览和 Windows E3 仍为 blocking。

Stage 5 实现旧 VN 兼容与现代化套件。AstraEMU Manager 仍是 Program target；legacy case 通过 `AstraEmuRuntimeProvider` 运行，每个 session 持有独立 `RuntimeWorld`。family 只注册 `LegacyRuntimeProvider` facade，私有 VM、VFS、媒体状态和诊断留在 provider session 内。Manager/RuntimeWorld 负责统一管理、文本翻译 Hook 和滤镜 preset；AstraEMU 不再提供 Luau patch/decode runtime。Family ABI 已 hard cut 到 v14，Product Runtime Provider 使用 ABI v4；旧 scene/snapshot/text/session-resource/budget 路径全部删除。Minori 的唯一合法组合是 `Native + MultiLayer`，必须通过同步 Hook、Host-owned surface、retained `Layer2D` transaction、writable-file port 和 Host-native menu/confirmation/text-input ports 工作。Minori consumer 已完成编译迁移，并恢复受限的签名真实样本 Headless slice；open 阶段现在可按显式 resource-audit policy 对全部 `.sc` 引用做有界完整校验。完整路线、Save 页注释视觉、checkpoint 语义、正式视觉/音频 review 和平台 E3 尚未闭合，因此 Stage 5 继续保持 `IN_PROGRESS`。

2026 年 8 月 23 日增量：动态 family loader、CLI 与 Manager 已绑定 ABI v9 四个 Host port，公共 support 层提供 retained surface store、私有 writable-file Host 和像素规范化。Minori resource/text 已走 `Native + MultiLayer`；文字先经过同步 translation Hook，再由 CosmicText 与 Astra Renderer2D 写入 Host surface。consumer implementation 基线为 `635527831e89e5ff9b87ac165b5b5532e28356c6`，Manager 使用公共 validator 和 WGPU 执行，不再使用字符串 binding 或 CPU fallback；旧 Scene2D transaction consumer 已删除。真实样本 slice 已输出 154 帧、非静音音频和零 diagnostic，但 checkpoint 标签与实际画面阶段仍有偏差，Stage 状态不变。

FVP provider 与动态 FFI 随后删除了 v9 已移除的 snapshot、text lease、session resource 和 step budget consumer，签名 probe/open/shutdown lifecycle 与 Manager adapter 定向测试通过。RFVP scene/text 尚未完成 Host surface 与 family-owned shaping 迁移，相关入口保持显式 blocker，因此仍只是 E1。

同一迁移已 rebase 到 `635527831e89e5ff9b87ac165b5b5532e28356c6`：filter graph 通过 ABI-owned typed DTO 进入 Product live layer，不再使用 string/hash binding。RFVP fork 更新到 `f4f64a5bb726c1759350a666a35e0a454b810f61`，局部 check 通过；Stage 证据等级不变。

FVP host-command media 已覆盖资源引用音频、流式 PCM、WMV/MPEG 与 Windows MP4 影片、fixed-tick frame selection、同 device wgpu composition、严格 `MediaFence` identity，以及 pending movie 的 family snapshot/rebind。runtime snapshot 使用有界压缩 envelope，并嵌入 live texture 的精确 RGBA；host-command audio restore 不重新读取 raw desktop VFS，而由 host 清理旧 stream 后通过 session resource channel 重建。Windows 本机授权样本的 ignored Headless run 已连续执行 188 tick、188 个 presented frame、10 条输入消息和一次 save/restore continuation，恢复后无扩展名 OGG 由 ABI encoding、合法扩展名和受控 magic 共同确定 codec，PCM 通过 Symphonia 增量 decoder 按 tick 提交，累计解码预算为 512 MiB，不再受 16 MiB whole-file PCM buffer 阻断。该 run 的 lifecycle、音频 meter、输入 trace、snapshot round-trip 与 redaction report 通过，但仍只属于 local-private Headless E2，不能替代逐帧 RFVP 对照或 Windows E3。同一样本也通过签名 development package 启动真实 Slint/WGPU 单窗口并保持响应，随后从 window close 走完 Manager/CLI shutdown；该次 native run 没有绑定自动输入、视觉变化、非静音 meter、route/terminal 和正式 run identity，仍只是 Windows E3 子链，不能关闭 E3。WMV/MPEG 使用增量 packet decoder；Windows MP4 video/audio 分别使用 stateful WMF SourceReader，按 PTS 合并后进入 16-frame/500ms 预取 ring 和可裁剪 PCM stream，并执行 running frame/byte/sample/timestamp budget。没有 public sanitized full-flow movie fixture 和真实 Windows/Android run identity，不能据此提升为 E3。

2026 年 8 月 3 日的真实安装原生 10 分钟 mixed-run 未通过性能与音频门禁。35,578 个 fixed tick 中，RFVP core p99 为 2.923 ms，完整 tick p99 为 16.690 ms、最大值为 5.892 s，device underflow 累计 656 次。第 651 step 的 effect dispatch 阻塞 3.940 s，而同 step 的 RFVP core 只用 2.696 ms。与 RFVP `0.5.0` 的 native renderer 对照后，根因边界已经收窄：原版按 `GraphBuff generation` 对同尺寸动态纹理执行原位 GPU 更新；hosted 路径仍把像素复制、序列化到 family ABI，随后分配新 scene resource generation，并至少完整重传变化纹理。通用 WGPU atlas 能增量复用空闲槽，只有容量不足或碎片化时才 repack；当前 trace 还不能把所有长帧归因于全 atlas rebuild。音频 producer 仍依赖 fixed tick 补充 120–180 ms device queue，所以图形长帧会直接造成撕裂。稳定 subresource update、独立 audio producer 和复跑通过前，本 Stage 的 FVP 性能、native audio 与 Windows E3 状态保持 `IN_PROGRESS`。

后续 clean Release 诊断已确认动态 VFS 小读放大是开屏等待的独立根因：加入 1 MiB 有界分页后，同一授权样本的 `runtime_open` 从 50,789 ms 降到约 220–400 ms；页尾跨页读取 panic 已修复并有真实越界形状回归。RFVP hosted 的脱敏 log record 也已改为通过 Family ABI v8 diagnostic DTO 到达可执行宿主，再由 `astra-observability` 输出；宿主消费后会清空 diagnostic，避免进入 Runtime output、save/replay/report 或状态 hash。修复后的 trace 进一步证明 present backlog 不是 RFVP core 或 WGPU draw 成本，而是 Windows PlatformHost 的 Tokio command queue 没有唤醒 Winit：单次 WGPU present 很短，命令却受 `about_to_wait` fixed polling 和 Windows timer 粒度支配。现在成功入队会经 `EventLoopProxy` 立即唤醒并由 `user_event` 排空，HTTPS completion 同样显式唤醒；Manager gamepad/metadata/translation completion 也改为 worker wake + UI-thread drain。800-step signed Release 已无 backlog，scene present 间隔中位数 16.677 ms、WGPU present p99 6.129 ms。标题 hover 动画、Family ABI scene bulk 零拷贝、10 分钟 CLI/Manager audio soak 与正式 E3 仍开放。

2026 年 8 月 8 日的颜色差异审计确认 texture filter 在 Family/Provider ABI 间丢失，同时证明 RFVP 上游原版渲染必须保持为视觉权威。此前改变 premultiplied-alpha 行为的尝试已撤销；hosted fork revision `a1d9abd201e6a0baf6259309543da5166a814c1c` 保留 typed ownership，并恢复上游原版 renderer。nearest/linear 现在逐层显式传递，FVP adapter 只把 NVSG bytes、vertex color、blend 与 filter 映射到 Astra 通用 draw state，不改写 Astra、Yakui 或 Minori 的 renderer。600 tick 同物理输入 Headless GPU 已通过，稳定标题帧相对上游 software oracle 的平均 RGBA MAE 为 2.211、最大 2.389，背景特效完整可见。相同 build/input 的第二次运行得到一致的 scene、raster 与 audio stream hash；Headless Kira 已改为 fixed tick 驱动，消除了 wall-clock 补水造成的 audio artifact overflow。Windows native GPU parity、Windowed E3 和同 revision Perfetto 尚未形成，因此不提升 Stage 状态。

2026 年 8 月 9 日的首线路复核把 hosted fork 更新到
`ce921717f043a8a035eadff1f41fc060c7de7c3c`。RFVP 的四个固定字体由 hosted fork 自己持有，
Astra 不再注入替代字体。上游 RFVP Headless oracle 与 AstraEMU Headless 使用同一物理输入，
均完成 33,682 帧。按 oracle 零基帧 `N` 对 Astra 一基 fixed step `N+1` 比较，dHash
p50/p95/p99/max 为 1/4/5/26，平均通道差异为 0/1/1/1，512 帧 RGBA SHA 完全一致。
原第 8,606 帧偏移的根因是 hosted core 漏掉上游在 dissolve 完成边沿、正常帧 tick 之前执行的
同步零时长 VM tick；修复留在 RFVP adapter，并持久化前一帧 dissolve 状态。完整线路已无语义
帧边界偏移，但软件栅格器仍有小幅像素差异，不能声明逐像素完全一致。最终 hosted revision 的
dirty 900 帧 Perfetto trace 实测 scene/PCM copied bytes 为零；I16 必要格式转换直接写入最终
Kira mix chunk，避免额外 F32 source buffer。Headless 未打开物理音频端点，确定性 Kira
mixer/service 的 underflow 为零。clean Release Windowed Perfetto 和人工 E3 仍是 Stage 门禁。

人工 E3 启动检查另行修复了 Manager auto-probe 未声明 `fvp.pack_paths` 的问题。Manager 只从
已绑定 VFS 中选择与 HCB 同目录的 `.bin`，生成唯一有序列表并注入本次 provider open；该列表
不写入 symbol-only runtime profile，也不会误收 save 子目录。Windows Kira worker 同时改用
无窗口、无 Winit event loop 的 media-service host 承载 audio/decode，Slint 保持进程内唯一窗口
event loop owner。开发复用 Release 分发包已通过签名、ABI、FVP probe、Runtime/Kira open，
真实 Manager 窗口保持响应；人工视觉、输入、音频、save/restore 和正常 shutdown 尚待确认。

2026 年 8 月 11 日，Minori 分支再次 rebase 到最新 `master`。长路线中共享 texture atlas 耗尽的根因不是容量不足，而是 character slot 在 stage 切换后永久存活；`.char keep` 现按原程序语义成为只跨下一次 `.stage` 的一次性 survivor marker。Headless execution budget 同时计入物理输入和全部有界 Await，deterministic host 只在 Headless 开启音频采集，并在 audio command 所属 fixed tick 内完成有界 VFS resource read，消除了 wall-clock 异步完成造成的 PCM 起点漂移。同一签名 plugin、八包 mount、profile 和序列化物理输入连续两次完成 63774 fixed steps、5314 个提交/栅格帧、15032 条输入、25 个 checkpoint、51018752 个 audio frame、snapshot round-trip、terminal 和零 diagnostic；input、VM、scene、raster、audio 与 terminal identity 全部一致。短程物理滚轮 E2 又以 126 fixed steps、126 个呈现帧、15 条输入和 3 个 checkpoint 验证 backlog 的 mode 1 panel、当前记录和关闭恢复，snapshot round-trip 成立且 diagnostic 为空。标题 Config 短程 E2 进一步以 190 fixed steps、190 个呈现帧、27 条输入和 3 个 checkpoint 验证 snapshot continuation 后的物理导航及真实 base 页面；修复点是把 source revision 与 resource URI 共同绑定为 texture revision，避免同 texture id 的页面被错误复用。Config 控件、标题完整路线、显式 save/load 和鉴赏仍未闭合。公共 Kira pre-master meter 的同路线诊断复跑测得 peak 1.849267、overload 6263 frame，首次超限时有 3 个 active stream，确认 clipping 来自多流叠加。原程序 bus headroom/limiter 尚未确认，不能先调低 master gain。Windows E3 同样开放，Stage 5 状态不变。

同日后续把全局进度接入 ordered `astra.platform.storage` provider request/result。payload 在 ABI 两侧受 1 MiB 上限约束；Headless 只用现有平台原子 transaction，Minori payload 只含四个已确认 clear flag 的 hash。真实标题启动路线连续两次均推进 31006 fixed steps、呈现 2662 帧并通过 30 个 checkpoint，覆盖 Config、backlog、用户 save/restore、真实影片、snapshot round-trip、terminal 和零 diagnostic。第二次运行增加一个 hash-only await，严格证明自然解锁数为 1；两次 visual、VM、terminal、coverage 和 audio hash 一致。模型检查全部 checkpoint 后未发现缺字、裁剪、拉伸、影片比例错误或图层残影。共享 Kira limiter 后 output peak 为 0.989551，output overload 与 underflow 均为 0。原版资源和已确认 clear 条件表明 `Memories` 只在第四条路线后开放，因此首条路线不伪造鉴赏入口；结局返回标题、鉴赏与 Windows E3 继续开放，Stage 5 状态不变。

2026 年 8 月 12 日补齐了结局返回标题语义。标题启动 session 的 `.end` 现在重建原版标题状态，direct entry 仍返回 terminal；返回标题后通过物理方向键与 Enter 执行 Exit。相同签名 plugin、mount、profile 和输入连续两次完成 31011 fixed steps、31627 个提交/栅格帧、16947 条输入、31 个 checkpoint 和 25277440 个 audio frame；snapshot round-trip、用户 save/restore、自然 unlock、返回标题、最终 Exit 和零 diagnostic 全部通过。VM、visual、terminal、coverage、scene、raster 与 audio identity 一致。独立真实脚本 slice 以严格 observation 捕获首个 choice 与进入实际分支后的 post-choice，choice 帧 hash 命中完整路线 fixed step 13928。模型检查 review bundle 要求的 frame，未发现缺字、裁剪、拉伸、影片比例错误或图层残影；WAV 自动量测确认无 clipping、静音全段或明显声道失衡，但涉及语音的整段试听尚未完成，`validate-review` 因 `ASTRA_HEADLESS_REVIEW_AUDIO_LISTEN_PENDING` 保持 blocking。`Memories` 仍遵守第四条 clear route gate；Config 控件、完整鉴赏和 Windows E3 继续开放，Stage 5 状态不变。

同日后续完成 `.char trans`/`.char vis` 的原程序语义复核与 typed runtime 接入。transition 由 fixed tick 线性推进并阻塞脚本，完整 continuation 状态进入 v22 snapshot；retained Scene2D 在等待期间持续输出。合成 256→128→0 透明度序列已通过 Headless WGPU capture 和人工视觉检查。真实样本 census 没有这两类命令，因此该证据不提升真实路线 coverage。旧 performance observer 未执行真实 120 Hz pacing，对应 run 已降级为诊断证据。后续将周期性长帧定位到过大的 GPU profile 在途窗口触发 `wgpu::Queue::submit` 背压；产品 observer 显式限制为两个在途 profile，PlatformHost 严格校验范围，不改变零 miss 预算。同一 clean Release build、package、profile、输入、adapter 与 driver identity 连续三次完成十分钟正式测量，每次 72000 个 sample 的 deadline miss 均为 0；runtime p99 为 0.2725–0.2938 ms，presentation p99 为 0.81456–1.11064 ms，trace 和资源预算全部通过。正式 GPU 性能 E2 已关闭；完整路线媒体、音频听审和 Windows E3 仍开放。

同日继续修复 restore 的两个根因。global progress 已启用、尚未发起首次 storage read 的新 session 是 Manager rollback snapshot 所需的合法静止态；它现在进入独立 `astra.emu.minori.global_progress_snapshot.v1` section，实际 pending I/O 仍不可保存。restore 后第一 tick 若同时完成旧 message wait 并产生下一条 message，provider 会把 retained gameplay scene 与文字放进同一 typed live transaction，不再让新 Headless host 在 0×0 GPU scene 上布局文字。修复后的同身份单次运行完成 33490 fixed steps、34108 帧、16951 条输入和 33 个 checkpoint，choice、post-choice、不可跳过的结局媒体、自然 unlock、返回标题与 Exit 都在同一通过报告中，diagnostic 为空。模型逐项查看全部 33 个 required frame；正式 audio listening 尚未完成，`validate-review` 正确以 `ASTRA_HEADLESS_REVIEW_BLOCKED` 阻断，Stage 5 状态不变。

正式 review contract 随后硬切到 v3。bundle 已要求完整 WAV，但旧 review 没有把 verdict 绑定到音频文件；v3 同时绑定 run report、review bundle 和 selected audio role/path/hash，CLI validator 还会复算实际 artifact hash。protocol、Python platform acceptance 和 release preflight 都会阻断缺失、额外、失败或 identity drift。v193 的 27260416 个 audio frame 已按连续区间生成 10 段 local-private 听审输入，catalog coverage 与源文件完全相等且 hash 匹配 bundle。具名逐段听审尚未完成，因此 Stage 5 状态不变。

2026 年 8 月 22 日，Minori Config 从静态 base 页面推进到 v23 状态层。原程序 29 类 action、鼠标命中区和滑块换算已经进入 draft transaction；`knob`、`checkmark`、`circle` 作为 retained overlay 与 base 合成。公共音频边界应用 BGM/Voice/SE 音量和静音，试听使用显式 WAV encoding，退出页面会停止专用 stream。定向测试和该 crate 的 114 项 library tests 通过。真实八包短程又以 166 fixed steps、171 帧、42 条物理输入和 6 个 checkpoint 验证 screen-effect checkmark、BGM knob 与 `BGMTest.wav` 试听；模型视觉审查通过，自动音频无 clipping/underflow。完整 WAV 未人工试听，正式 review 保持 blocking；全屏 Host effect、逐字速度、视觉开关对剧情的实际影响、角色语音筛选和 Windows E3 仍开放，Stage 5 保持 `IN_PROGRESS`。

同日，持久 Auto 的阻断定位到活动消息仍保留旧 `Input` await，而不是 tick 预算不足。VM 现在以同一 token 做 `Input`/`Time` modality 重绑定；CLI 与 Manager Host 只允许这组互换，其他重复 token 继续阻断。Config 的 Auto 速度 `0` 映射为一个 10 ms timing unit，不放宽公共正时长约束。真实八包首路线随后在正文阶段不发送周期性 Enter，仅在 choice active 后确认一次，共推进 25552 fixed steps、提交并栅格化 25557 帧、消费 50 条物理输入并到达 terminal；snapshot round-trip、既有 coverage identity 和零 diagnostic 成立。人工查看六个关键 checkpoint 未发现视觉阻断。该结果关闭持久 Auto 首路线 E2，不关闭 Skip 整路线、正式音频听审、Config 剩余行为、鉴赏或 Windows E3，Stage 5 仍为 `IN_PROGRESS`。

同一等待契约随后覆盖受 pragma gate 约束的 Control 快进。按下 Control 会把已显示消息重绑定为 10 ms `Time`，释放后若等待尚未完成则恢复为 `Input`；媒体与 provider fence 不会被自动完成。真实八包首路线从标题前按住物理 Control，正文阶段没有周期性 Enter，共完成 25496 fixed steps、25498 帧和 28 条输入；terminal、snapshot round-trip、自然解锁、既有 coverage identity 和零 diagnostic 均成立。音频 master peak 为 0.989372，overload/underflow 为 0；三个关键 checkpoint 的人工检查未发现视觉阻断。这关闭 Control 首路线 E2。

持久 Skip 的首次真实运行进一步发现 Config 左侧 slider x 区间会吞掉下方 Auto/Skip 与部分 Font hit region。修复后的分派只在 slider y 范围命中 slider，其余坐标进入非 slider hit map。第二次运行从 Config UI 选择 Skip 并 Apply，启动后释放 Control，再从游戏菜单启用 Skip；严格状态 observation 通过。完整首路线推进 24901 fixed steps、24906 帧和 52 条输入，terminal、snapshot round-trip、自然解锁、既有 coverage identity、零 diagnostic 与自动音频门禁均通过。六个 checkpoint 的人工检查确认 Skip 单选标记和主要画面没有视觉阻断。持久 Skip 首路线 E2 至此关闭；正式音频听审、Config 剩余行为、鉴赏和 Windows E3 仍开放。

backlog 多记录 E2 随后暴露 retained text 的生命周期错误：未变化的 system tick 仍清除文字，却不会重发 scene/lease。清除动作现只绑定页面、cursor、输入、restore 或 terminal 的实际变化。VM 三记录 cursor 钳制和 provider idle retention 回归通过；真实八包短程以 771 fixed steps、776 帧、55 条输入验证打开、连续两次上翻和关闭，snapshot round-trip 与零 diagnostic 成立。人工查看确认三条历史文字及 gauge 位置逐次变化，关闭后恢复当前消息。该短程未到 terminal，只关闭 backlog 多记录翻页的定向 E2。

Config 文字阴影随后接入既有 typed text presentation。启用时使用已验证的 2 px 黑色 outline，关闭时提交 `None`；CosmicText shaping、字体绑定、layout 和 Renderer2D 合成保持公共主路径，没有 family 私有 renderer 或 fallback。真实八包短程以 384 fixed steps、388 帧、34 条物理输入验证 Config 开关、Apply 和首条剧情消息，snapshot round-trip 与零 diagnostic 成立。人工视觉检查未见缺字、裁剪、错层或残影。该短程未到 terminal，只关闭文字阴影定向 E2，Stage 5 仍为 `IN_PROGRESS`。

后续 Headless 修正了按采样间隔保留 surface 导致的 stale checkpoint：每个显式 checkpoint 都会先提交待处理 Scene2D 并排空 receipt，Config 与 backlog 的真实短程截图现能分别证明页面打开和关闭。decoded video 通过公共 `SceneCommand::VideoFrame` 合成；脚本未标为 skippable 的首轮 movie 不接受 Control 跳过。真实影片 checkpoint 进一步修正了 transient draw blend 和公共 WGPU atlas 连续帧 placement 生命周期；人工查看确认 decoded frame 非空、比例正确，剧情层文字合成无裁剪或旧帧残留。公共 Kira main track 使用成熟 Compressor 组成显式 peak limiter，并在前后各保留 meter。完整路线记录约 2.71 的 pre-master peak，但 master output peak 约 0.990、overload 为 0、underflow 为 0，完整 WAV 非静音且低于 i16 full scale；blocked machine report 写完后返回非零退出码。标题启动路线完成 28814 fixed steps、2480 个呈现帧、20058 条物理输入、30 个 checkpoint、snapshot、用户 save/restore 和 terminal。该证据仍缺自然鉴赏解锁和 Windows E3，Stage 5 继续保持 `IN_PROGRESS`。

clear flag 的处理现已收紧到原程序确认的四个精确名称。runtime 只保存对应的脱敏 unlock identity，snapshot restore 会校验集合边界；provider 只报告 session 内 unlock count。任意未知 `CLEAR` 名称都会被忽略。平台 global progress 的原子提交、新 session 恢复和标题资源切换尚未实现，因此这项局部证据不会提升自然鉴赏或 Windows E3 状态。

Windows E3 harness 已作为 `publish = false` 的 `astra-emu-e3` 接入 workspace。它只从 ignored 本地 manifest 读取授权 source、entry、`astra.user_input_sequence.v1` JSONL 和私有输出目录，启动实际 Manager 可执行文件，以 Win32 `SendInput` 重放键盘、鼠标和滚轮，并对窗口、焦点、客户区、逐事件捕获与超时严格失败。harness 使用独立 Manager 数据目录，不污染日常资料库；可提交摘要只保留 schema、hash、计数、生命周期和 diagnostic。Manager 现记录已消费输入、terminal、输出音频 meter、session/package/profile identity 与正常 shutdown 的脱敏事件；游戏页提供真实用户可用的 F5 save、F9 restore，且只调用 RuntimeWorld/provider 的 save/restore lifecycle。harness 在真实关闭路径后读取这些事件，不把进程 kill 当作 shutdown。FVP coverage 与同 run build identity 的完整外部证据仍未形成，任一缺失继续 blocking，不能输出成功 E3。因此仍为 `IN_PROGRESS`；授权样本尚未运行时，不产生 Windows E3 证据。

## S5-GAME-RUNTIME-01 AstraEmuRuntimeProvider gameplay runtime

**ID:** `S5-GAME-RUNTIME-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU 作为 `AstraEmuRuntimeProvider` 与 `NativeVnRuntimeProvider`、后续 `AstraRpgRuntimeProvider` 同级接入，不直接替换 `RuntimeWorld`。

**Depends On:** `S2-VFS-01`、`S3-RUNTIME-PROVIDER-01`、[Game Runtime Provider Contract](../../contracts/game-runtime-provider.md)、[Game Runtime Provider Blueprint](../../implementation/game-runtime-provider.md)

**Target Paths:** `Emulator/Source/Manager/astra-emu-manager-core/src/runtime_provider.rs`、`Emulator/Source/Manager/astra-emu-manager/src/main.rs`

**Steps:**

1. 定义 `AstraEmuRuntimeProvider` descriptor、prepare/probe/open/step/save/restore/shutdown、package section plan、release checks 和 editor metadata。
2. 让 case target 显式绑定 `astra_emu` runtime provider；Manager 只负责 program shell、profile、UI 和 local operator workflow。
3. `open` 创建 RuntimeWorld lifecycle StateMachine，并选择 family `LegacyRuntimeProvider` session。
4. `step` 调用 family provider，原子提交轻量 `LegacyControlTransaction`，再把 typed scene、PCM、text、video、wait、trace 和 diagnostic 移动给 host owner。
5. Release Gate 校验 `emu.game_runtime_provider`、provider fingerprint、package sections、save/replay hash 和 report redaction。

**Done Evidence:** `cargo test -p astra-emu-manager game_runtime_provider` 和 `cargo test -p astra-release emu_gate` 通过；report 输出 `emu.game_runtime_provider`，且 family plugin 仍不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

**Linked Test IDs:** `T-S5-GAME-RUNTIME-01`

## S5-EMUCORE-SM-01 EmulatorCore VM state-machine mapping

**ID:** `S5-EMUCORE-SM-01`

**Status:** `IN_PROGRESS`

**Goal:** Family 内部把旧 VM 映射为私有 scheduler、context、basic-block 和 action 状态机，公共 Runtime 只接收 typed control transaction，host 直接接收 owned live output。

**Depends On:** `S5-GAME-RUNTIME-01`、`S5-FAMILY-01`、[EmulatorCore StateMachine Mapping](../../implementation/emulator-core-state-machine.md)

**Target Paths:** `Emulator/Source/FamilyApi/astra-emu-family-api/src/lib.rs`、pinned `rfvp` hosted fork、`Emulator/Source/Families/astra-emu-fvp/src/provider.rs`

**Steps:**

1. 定义 family-private scheduler trace、context id、sequence、budget、wait/yield/fault/terminal 状态和 snapshot cursor。
2. 多线程、多 fiber 或多 context VM 使用 child state machine，并按固定 `(priority, context_id, sequence)` 推进。
3. Basic block 执行到 syscall、branch、wait、fault 或预算耗尽时停止，并输出 action trace。
4. Syscall/action bridge 只输出 typed scene、PCM、text、video、wait、control transaction 和 diagnostic。
5. 编写 scheduler ordering、await boundary、snapshot/replay hash、fault isolation 和 FVP detailed mapping 测试。

**Done Evidence:** `cargo test -p astra-emu-family-api family_scheduler` 和 `cargo test -p astra-emu-fvp state_machine_mapping` 通过；report 输出 `emu.vm_state_machine_trace`、context coverage、await boundary 和 replay hash。

**Linked Test IDs:** `T-S5-EMUCORE-SM-01`

## S5-LEGACY-VFS-01 Legacy pack VFS mounts

**ID:** `S5-LEGACY-VFS-01`

**Status:** `IN_PROGRESS`

**Goal:** 所有 family pack reader 复用 Asset VFS，旧引擎 pack 只作为 `legacy_pack` mount source，不能替代 `.astrapkg`。

**Depends On:** `S2-VFS-01`、`S5-GAME-RUNTIME-01`、[Asset VFS Contract](../../contracts/asset-vfs.md)

**Target Paths:** `Emulator/Source/FamilyCore/astra-emu-family-core/`、`Emulator/Source/FamilySupport/astra-emu-family-support/`、`Emulator/Source/Families/astra-emu-fvp/src/archive.rs`、`Emulator/Source/Families/astra-emu-minori/`、`Emulator/Source/Programs/astra-emu-cli/src/vfs.rs`、`Emulator/Source/Programs/astra-emu-minori-cli/`

**Steps:**

1. 为 Artemis PFS、FVP `.bin`、KrKr XP3、BGI PackFile、Siglus Scene.pck、SoftPAL PAC/DAT 和 Minori PAZ 定义 `vfs_provider` capability 和 legacy prefix，例如 `fvp:/...`。
2. Pack reader 输出 entry table hash、`VfsUri`、entry id、offset、size、hash、media kind、compression support 和 diagnostic。
3. Overlay mount 只允许 profile 声明的 key pattern；同 key 多命中没有 allowlist 时 blocking。
4. `.astrapkg` 保存 case profile、reader identity/hash、release report 和 sanitized scenario refs，不保存商业 payload。
5. Release Gate 校验 entry bounds、hash、unsupported compression、reader identity、path/payload redaction 和 package/source consistency。

**Done Evidence:** `cargo test -p astra-emu-family-api legacy_pack_vfs` 和 `cargo test -p astra-release emu_gate` 通过；report 输出 `emu.legacy_pack_vfs`，且不写本地 root、payload、完整脚本或 bytecode。

**Current Evidence:** in-process VFS 位于 `astra-emu-family-core`，profile/viewer/verify/extract/FUSE 位于 `astra-emu-family-support`。2026-08-29 已从 AstraEMU Luau/private-profile/plaintext-cache 硬切到严格 `key.toml` 与 family-owned 流式解密；GARbro importer、NRBF reader、patch UI/evidence 和 `windowed-e2` 同步删除。重新扫描确认 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive，其中 `bg` 由主包和 A–J 分卷组成，全目录共 18 个物理 PAZ 文件。当前 manifest v4/key-file identity 的八包 full verify 已覆盖 14502 entries 和 6624958365 decoded bytes，脚本/media census 也已重跑；同一 v4 Release 包的长程 Headless E2 已通过 193 条输入、24048 个 fixed step、48 个 checkpoint、146 个提交帧和 16758784 个音频帧，诊断为空且尾部显式 shutdown。四路线自然结局、同点原版视觉、Release CLI Sandbox 视觉验收、Linux FUSE、macOS extract、峰值内存规模证据和正式 Windows E3 仍待完成，因此本项保持 `IN_PROGRESS`。

2026-08-26 follow-up：Manager 的 Minori AVI streaming 与 first-frame preview 已统一到 AstraMedia 的显式增量契约；Manager/Headless 在 composition root 直接注册 `astra.decode.ffmpeg.incremental`，Minori 只保留 AVI 扩展名/RIFF 身份和 preview binding。Minori video extension 在进入 FVP/Windows provider 前严格只接受 `avi`，其他 codec 返回 `ASTRA_EMU_MINORI_VIDEO_CODEC_UNSUPPORTED`。生产路径通过显式 `ffmpeg-vcpkg` 绑定使用 FFmpeg 的 demux/codec，`wmv-decoder`/手写 AVI 路径已从 Minori 和 CLI 依赖移除。未编译 FFmpeg 时直接返回 blocking diagnostic，不回退到平台 codec。真实样本 Headless media slice 已完成 60 fixed ticks、16 帧、111104 音频帧、非静音和零 diagnostic；这仍只是 provider-boundary/E2 evidence，未生成 Windows E3 或完整路线 artifact。

**2026-08-09 identity/choice follow-up:** 当前 Family ABI 已 hard cut 到 v8，Provider ABI 已 hard cut 到 v4；上段 v7 动态运行只保留为历史 evidence。v8 增加 retained ephemeral text 的水平对齐和显式清除，v5/v6/v7 plugin identity 均拒绝。Minori 选择界面已按原程序的最多四项、blur/focus/active 资源、26 px 字体、纵向排列和整体居中接入 resource scene 与批量 CosmicText lease，确认后清除 retained glyph。相关 parser/VM/provider/Host 增量测试已通过；真实路线尚未生成 v8 选择 checkpoint，因此不能关闭完整路线或系统 UI 门禁。

FVP 补充证据：FVP 与 Minori factory 由 CLI/Manager 显式注册。FVP factory 覆盖 profile 明列的 root `.bin`、manifest v2、NLS、目录/stat、有界 range、按需 stream、source revision、重复 range 账本和正式全源审计；损坏包、重叠 range 与不受支持的 private patch 均会阻断。同一授权样本已完成 12 archive、18166 entry 的单遍有界 mount/list；这是 local-private VFS evidence，不替代 Release 性能与全资源 verify。

**Linked Test IDs:** `T-S5-LEGACY-VFS-01`

## 2026-08-25 当前 Minori v9 smoke 证据

重新安装 stable Rust 1.98 后，当前签名 Minori plugin 通过 v9 `Native + MultiLayer` composition 完成 431 个 fixed step、27 条物理输入、13 个 submitted/rasterized frame 和 344576 个 audio frame；Headless report 为 `passed`，无 diagnostic，标题与场景 frame hash 不同。该 run 未到达 terminal，只是 E2 smoke，不代表完整路线、gallery、正式音频审查或 Windows E3 已完成。旧 gallery 输入在当前 identity 下触发 `ASTRA_EMU_HEADLESS_CHECKPOINT_AFTER_TERMINAL`，已从证据集中排除，需重新生成匹配当前 global-progress 的物理输入。

## S5-MANAGER-01 Manager RuntimeWorld bridge

**ID:** `S5-MANAGER-01`

**Status:** `IN_PROGRESS`

**Goal:** Manager 能启动 `AstraEmuRuntimeProvider`，由 provider 创建 RuntimeWorld、启用 family plugin、打开 `LegacyRuntimeProvider` session、驱动生命周期 StateMachine，并输出 local case report。

**Depends On:** `S5-GAME-RUNTIME-01`、`S5-EMUCORE-SM-01`、`Docs/contracts/astraemu-ipc.md`、`S1-CORE-01`、`S1-PLUGIN-01`

**Target Paths:** `Emulator/Source/Manager/astra-emu-manager-core/src/runtime_provider.rs`、`Emulator/Source/Manager/astra-emu-manager/src/family_host.rs`、`Emulator/Source/Manager/astra-emu-manager/src/main.rs`、`Emulator/Source/Programs/astra-emu-cli/`

**Steps:**

1. 定义 case launch request、profile、runtime provider binding、family selection、`LegacyRuntimeHostCtx` binding 和 report destination。
2. 启动 `AstraEmuRuntimeProvider`，加载项目 package 或 synthetic fixture，启用 selected family plugin。
3. 通过 `AstraEmuRuntimeProvider::open` 建立 RuntimeWorld 和 family session，并让生命周期 StateMachine 在固定 tick 调用 `emu.step`。
4. 建立 input、overlay、diagnostics、TextCaptureEvent 和 presentation/audio command 采集路径。
5. 编写 plugin disabled、permission denied、missing provider、session fault 和 report redaction 测试。
6. 提供显式 family/game directory 的 quick launch，以及只消费物理输入 JSONL、复用 `astra-platform-headless` 的自动化入口；不得旁路 RuntimeWorld 或 family lifecycle。

**Done Evidence:** Manager 不解析 family 私有 VM 内存，不持有 family 文件系统、renderer/audio handle 或 Actor 指针；所有玩家可见输出都来自 `AstraEmuRuntimeProvider` 输出到 RuntimeWorld 的 event/presentation/audio/report。

**Linked Test IDs:** `T-S5-MANAGER-01`、`T-S5-EMU-CLI-01`

## S5-MANAGER-UI-01 Slint Manager 与 runtime overlay

**ID:** `S5-MANAGER-UI-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU Manager、diagnostic/translation/filter overlay 使用 Slint 1.17.1；host 统一持有 winit 0.30 event loop、surface 与 wgpu 29.0.4 device/queue，并复用 shared UI input、semantic、resource 和 render contract。

**Depends On:** `S5-MANAGER-01`、`S2-UI-BACKEND-01`、[ADR 0015](../../adr/0015-ui-backend-provider-split.md)

**Target Paths:** `Emulator/Source/Manager/astra-emu-manager-ui-slint/`、`Emulator/Source/Manager/astra-emu-manager/`

**Steps:**

1. 使用 Astra design tokens 和 Slint 组件实现桌面三栏、手机双列/bottom sheet/bottom navigation、移动大屏桌面式布局和游戏 overlay，不让 Slint 类型进入 Manager Core 或 family API。
2. Slint rendering notifier 取得同一套 wgpu 29 `Device`/`Queue`，Astra renderer 直接提供 GPU stage texture；禁止 CPU 整帧回读和跨设备复制。
3. 完成 keyboard/gamepad/touch/IME、focus、screen reader semantics、safe area、overlay input consumption、surface rebuild 和 device loss。
4. Windows 与 Android GPU emulator 形成 E3；Linux、macOS、iOS 关闭 package/provider/host compile E2；Web 对 native family plugin 返回稳定不支持诊断。
5. About 显示 Slint Royalty-free 2.0 规定归因并随包维护第三方 notices。

**Done Evidence:** Windows/Android Manager workflow、同设备 WGPU identity、响应布局、overlay input isolation、report redaction、accessibility 和 provider identity 通过真实程序证据；不能用静态面板、compile-only 或 emulator 结果外推未验证硬件。

**Current Evidence:** 当前 active family ABI 为 v9；v5/v6/v7/v8 仅作为 fail-fast rejection 或迁移历史输入。v9 保留规范键名输入契约（废弃语义动作），手柄/键盘重映射作为 Manager 层通用能力落地：`InputMapping`（schemars 导出）+ 通用 VN 预设 + 开关/死区 + 逐按键绑定 UI，Library v9 `input_settings`/`work_settings` 支持全局与逐游戏输入映射覆盖（启动生效、离开恢复）。该部分为 crate check 与 unit/集成测试级证据；Windows/Android 真实手柄 E3 仍未闭合。

**Linked Test IDs:** `T-S5-MANAGER-UI-01`

## S5-FAMILY-01 LegacyRuntimeProvider facade

**ID:** `S5-FAMILY-01`

**Status:** `IN_PROGRESS`

**Goal:** 定义并实现 `LegacyFamilyPluginDescriptor`、`LegacyRuntimeProvider`、`LegacyRuntimeSessionId`、`LegacyRuntimeHostCtx`、`LegacyStepInput`、`LegacyStepOutput`、`LegacyLiveOutput`、`LegacyControlTransaction`、`LegacyWaitRequest` 和 `LegacySnapshotEnvelope`。

**Depends On:** `S5-GAME-RUNTIME-01`、`S5-EMUCORE-SM-01`、`S5-LEGACY-VFS-01`、`S5-MANAGER-01`、`Docs/contracts/astraemu-ipc.md`、`Docs/implementation/astraemu-legacy-runtime-framework.md`、`Docs/implementation/provider-plugin-api.md`

**Target Paths:** `Emulator/Source/FamilyApi/astra-emu-family-api/src/lib.rs`、`Emulator/Source/Manager/astra-emu-manager-core/src/family_loader.rs`

**Steps:**

1. 定义 family descriptor、runtime provider id、format capability、permission、failure classification 和 redaction policy。
2. 定义 lifecycle API：`probe`、`open`、`step`、`save`、`restore`、`shutdown`；`open` 返回 session id，provider 负责区分并行 case。
3. 定义 provider DTO，稳定 ID、revision、section ref、source span、capability diagnostic 和 typed ABI-owned bulk 分离；实时 scene/PCM 不携带 content hash 或 postcard payload。
4. 让 `step` 返回 typed owned live output 与轻量 control transaction；host 先原子提交 control，再消费式移动 scene/PCM，不建立完整镜像。
5. 编写 provider registration、session lifecycle、typed live ownership、snapshot envelope、restore compatibility 和 redaction 测试。

**Done Evidence:** family plugin 不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks，family VM state 只存在于 provider session。

**Linked Test IDs:** `T-S5-FAMILY-01`

## S5-AUTOPROBE-01 Manager auto probe

**ID:** `S5-AUTOPROBE-01`

**Status:** `IN_PROGRESS`

**Goal:** Manager 能按固定 family 优先级自动 probe case，并允许用户用 profile 手动覆盖。

## S5-METADATA-01 作品识别、元数据、游玩记录与兼容性库

**Status:** `IN_PROGRESS`

**Goal:** 本地扫描与 family probe 保持独立，同时建立作品级身份、VNDB/Bangumi metadata provider、可解释确认队列、Bangumi 收藏状态同步、本地游玩时间/历史统计与社区中央兼容性库匹配。

**Target Paths:** `Emulator/Source/Providers/astra-emu-metadata/`、`Emulator/Source/Providers/astra-emu-metadata/src/compatibility.rs`、`Emulator/Source/Manager/astra-emu-manager-core/src/identity.rs`、`Emulator/Source/Manager/astra-emu-manager-core/src/play_record.rs`、`Emulator/Source/Manager/astra-emu-manager-core/src/compatibility_cache.rs`、`Emulator/Source/Manager/astra-emu-manager/src/metadata_runtime.rs`、`Emulator/Source/Manager/astra-emu-manager-ui-slint/`

**Current Evidence:** Library v10 在 v6 work/installation/external identity/snapshot/candidate/decision/scan run/consent/Bangumi state 基础上新增 `play_session`、`compatibility_entry_cache`、`compatibility_sync_state`、`vn_release`、`case_release` 等表；`play_record.rs` 提供会话开启/结算/崩溃残留结算与 SQL 聚合统计，`compatibility_cache.rs` 提供缓存替换、同步状态、VNDB 发布（rID）缓存与逐版本匹配。`astra-emu-metadata/src/compatibility.rs` 定义五级 `astra.emu.compatibility.v2` schema——**VNDB 是唯一权威源**，条目按 `(vID, rID)` 键控，兼容性精确到特定游戏版本；Rust 类型为真源，`schemars` 导出 JSON Schema，HTTPS-only 拉取客户端与 SHA-256 增量同步。Manager 在 launch/leave_game/shutdown 埋点计时，经 `MetadataRuntime` worker 刷新兼容性并 materialize 到本地缓存；VNDB provider 可拉取某 work 的 release（rID）列表，用户可把本地安装钉到具体版本，UI 在 inspector 显示 vID/rID 与逐版本分级；Slint UI 提供封面卡适配徽章、按分级筛选、inspector 兼容性详情与 Settings → Compatibility 子页。社区通过 `.github/ISSUE_TEMPLATE/vndb-game-compatibility.yml` 提交结构化 vID/rID 报告，维护者合并进数据仓。迁移、play session 聚合、崩溃结算、逐版本匹配 join、serde 往返与 JSON Schema 导出的 unit/跨模块测试属于 E1/E2；真实网络拉取、中央数据仓、完整 Manager UI 自动化、正式 release license gate 和 Windows/Android E3 仍未闭合，因此不能标记 `DONE`。

**Done Evidence:** v5 回滚迁移、离线 provider contract fixture、取消与恢复、冲突确认、拒绝记忆、手动 ID、封面边界、Bangumi 收藏更新、桌面/窄屏 UI、商业 VNDB license gate 和 observability redaction 全部通过；在线或 UI fixture 最高只计 E2。

**Depends On:** `S5-MANAGER-01`、`S5-FAMILY-01`

**Target Paths:** `Emulator/Source/Manager/astra-emu-manager-core/src/probe.rs`、`Emulator/Source/Manager/astra-emu-manager/src/main.rs`

**Steps:**

1. 定义 `FamilyAutoProbePolicy`，默认顺序为 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori。
2. 让 Manager 逐个调用 family `probe`，收集 marker、confidence、blocker 和 skipped reason。
3. 支持 case profile 显式指定 family/profile，并在 report 中记录 override reason。
4. 无命中或全部 blocker 时进入手动选择，不尝试执行商业脚本。
5. 编写 synthetic multi-family marker、manual override 和 no-match report 测试。

**Done Evidence:** 自动选择结果可复现，report 能解释命中、跳过、覆盖和最终 family。

**Linked Test IDs:** `T-S5-AUTOPROBE-01`

## S5-TEXT-01 Text dump and translation provider

**ID:** `S5-TEXT-01`

**Status:** `IN_PROGRESS`

**Goal:** `TextCaptureEvent` 进入 Manager 文本管线；首发 translation provider 通过 ECNU Open API 的显式 Responses/SSE 或 Chat Completions profile 更新非权威 overlay，并执行 consent、预算、缓存、secret 与 report redaction policy。

**Depends On:** `S5-MANAGER-01`、`S5-FAMILY-01`、`S4-AI-01`、`S4-AI-04`

**Target Paths:** `Emulator/Source/Providers/astra-emu-translation-openai-compatible/`、`Emulator/Source/Manager/astra-emu-manager-core/src/library.rs`

**Steps:**

1. Profile 必须显式填写 endpoint、protocol、model、目标语言、上下文 0–32、正文预算和 secret reference；代码不硬编码默认 model，也不在失败后切换 endpoint/protocol/model。
2. 默认最近 10 句，总正文上限 16 KiB；背景、术语表和上下文超限时在句边界确定性截断。
3. 全局一次授权永不自动失效；UI 始终显示 endpoint、model 和发送范围。默认只有 session cache，用户按游戏 opt-in 后才写 SQLite。
4. timeout、限流、transport 和协议错误不阻塞 Runtime；保留原文、记录稳定 diagnostic、有限退避后熔断，只允许用户手动恢复。
5. shipping credential 只存平台 secret store；SQLite、日志、report、save/replay 和 package 只保存 secret reference 或 hash/count/latency/error code。

**Done Evidence:** Responses SSE、显式 Chat adapter、截断、consent、cache、timeout/rate-limit/circuit breaker 与 redaction 测试通过；另有 ignored live test 使用 ignored env，并证明凭据未进入输出。overlay 不改变 runtime replay hash。

**Linked Test IDs:** `T-S5-TEXT-01`

## S5-FILTER-01 AstraEMU FilterGraph presets

**ID:** `S5-FILTER-01`

**Status:** `IN_PROGRESS`

**Goal:** AstraEMU 复用引擎 `FilterGraph`，为旧 VN case 绑定 final-frame 和 per-layer filter preset。

**Depends On:** `S2-MEDIA-04`、`S5-MANAGER-01`

**Target Paths:** `Emulator/Source/Manager/astra-emu-manager-core/src/filter.rs`、`Emulator/Source/Manager/astra-emu-manager/src/stage_renderer.rs`

**Steps:**

1. 定义 `EmuFilterPresetBinding`，包含 final-frame preset 和可选 per-layer role preset。
2. final-frame preset 对 RuntimeWorld 合成后的画面做后处理。
3. per-layer preset 绑定 `PresentationCommand` 的 layer id 或 role；family 缺少 layer metadata 时只启用 final-frame。
4. 输出 missing layer metadata diagnostic，不新增 family 专属 shader/filter API。
5. 编写 final-frame、per-layer、metadata 缺失和 headless hash 测试。

**Done Evidence:** filter preset 使用同一 `FilterGraph` contract；family plugin 不直接持有 renderer handle 或 shader object。

**Linked Test IDs:** `T-S5-FILTER-01`

## S5-ARTEMIS-01 Artemis family plugin

**ID:** `S5-ARTEMIS-01`

**Goal:** Artemis family plugin 支持 PFS/PF6/PF8 probe、boot keys、`.iet` tag、legacy Lua call/filter、presentation/media command、snapshot 和 report。

**Depends On:** `S5-FAMILY-01`、`Docs/emu/artemis/implementation-checklist.md`

**Target Paths:** `Emulator/Source/Families/astra-emu-artemis/`、`Emulator/Tests/artemis/`、`scenarios/emu/artemis_full_flow.yaml` planned target

**Steps:**

1. 实现 PF6/PF8 header、index、entry bounds check、PF8 XOR 和 patch chain resolver。
2. 读取 `system.ini` boot keys，选择 platform section 和 BOOT entry。
3. 解析 `.iet` text/tag、legacy Lua block hash、`.ast` table row 和 ASB classification。
4. 接入 tag filter、enqueueTag、presentation/media command、AwaitToken 和 serializable snapshot allowlist。
5. 编写 synthetic PFS、boot metadata、tag parser、snapshot replay 和 full-flow scenario 测试。

**Done Evidence:** Artemis report 不含商业 payload、私有绝对路径、未授权截图、音频采样或完整脚本。

**Linked Test IDs:** `T-S5-ARTEMIS-01`

## S5-KRKR-01 KrKr family alpha profile

**ID:** `S5-KRKR-01`

**Goal:** KrKr family 输出 alpha probe profile，验证 XP3 probe、virtual storage、script classifier、KAG boot trace、media bridge 和 release report。

**Depends On:** `S5-FAMILY-01`、`Docs/emu/krkr/implementation-checklist.md`

**Target Paths:** `Emulator/Source/Families/astra-emu-krkr/`、`Emulator/Tests/krkr/`、`scenarios/emu/krkr_probe.yaml` planned target

**Steps:**

1. 实现 XP3 index、patch layering 和 virtual storage resolver。
2. 识别 KAG source、TJS bytecode、`.ks.scn`/PSB binary scenario，并为 unsupported branch 输出 diagnostic。
3. 输出 image、voice、BGM、movie command probe 和 boot trace hash。
4. 编写 synthetic fixture、metadata smoke 和 probe scenario 测试。

**Done Evidence:** KrKr alpha report 不含商业 payload、私有绝对路径、未授权截图或音频采样。

**Linked Test IDs:** `T-S5-KRKR-01`

## S5-BGI-01 BGI family plugin

**ID:** `S5-BGI-01`

**Goal:** BGI family plugin 支持 PackFile/BURIKO ARC20、DSC decode、BCS/BP probe、VM memory、host dispatch、media probe 和 report。

**Depends On:** `S5-FAMILY-01`、`Docs/emu/bgi/implementation-checklist.md`

**Target Paths:** `Emulator/Source/Families/astra-emu-bgi/`、`Emulator/Tests/bgi/`、`scenarios/emu/bgi_full_flow.yaml` planned target

**Steps:**

1. 实现 archive index、bounds check、name normalization 和 DSC decode。
2. 实现 BCS、BP、headerless scenario 检测顺序和 parser。
3. 实现 VM memory、stack、PC、program table 和 source map。
4. 实现 Host dispatch diagnostic、AwaitToken、Presentation、Image/Audio/Movie probe。
5. 编写 archive fixture、script fixture、VM dispatch 和 full-flow scenario 测试。

**Done Evidence:** BGI local report 只输出 hash、offset、entry count、opcode histogram 和脱敏 metadata。

**Linked Test IDs:** `T-S5-BGI-01`

## S5-SOFTPAL-01 SoftPAL 接入门槛

**ID:** `S5-SOFTPAL-01`

**Goal:** SoftPAL 在首批 family 稳定后接入，先完成 probe、resource catalog、script VM、extcall diagnostics 和 release gate。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/softpal/implementation-checklist.md`

**Target Paths:** `Emulator/Source/Families/astra-emu-softpal/`、`Emulator/Tests/softpal/`、`scenarios/emu/softpal_full_flow.yaml` planned target

**Steps:**

1. 复用 `LegacyRuntimeProvider` facade，不新增 Manager 私有通道。
2. 实现 PAC/DAT probe、resource catalog 和 script VM alpha route。
3. Unknown extcall 默认输出 diagnostic；presentation/audio/save/control-flow side effect 缺失时 release gate 不算通过。
4. 编写 fixture smoke、extcall report 和 full-flow scenario 测试。

**Done Evidence:** SoftPAL gate 能区分 recoverable diagnostic 和阻断玩家流程的 missing extcall。

**Linked Test IDs:** `T-S5-SOFTPAL-01`

## S5-FVP-01 FVP 接入门槛

**ID:** `S5-FVP-01`

**Status:** `IN_PROGRESS`

**Goal:** FVP 作为 v1 首发 family，以固定 rfvp revision 为行为基线，覆盖 probe、archive/media resolver、完整 HCB VM/syscall、presentation/audio/movie/input 与 save/load/snapshot/replay。

**Depends On:** `S5-FAMILY-01`、`S5-GAME-RUNTIME-01`、`S5-LEGACY-VFS-01`、`Docs/emu/fvp/implementation-checklist.md`

**Target Paths:** `Emulator/Source/Families/astra-emu-fvp/`、pinned `rfvp` hosted fork、`Tools/verify_fvp_parity.py`

**Steps:**

1. 固定并记录 rfvp revision、MPL-2.0 notice、修改记录与 source offer；合法输入逐字节对齐 parser、0x00..0x27 opcode、Variant、stack/call frame、context/thread request、read-state 和 syscall 可观察行为。
2. 实现 `.bin` VFS、HZC1/NVSG、Ogg/RIFF、WMV/MP4 compatibility probe、cursor、graph/prim/text/audio/movie/input/save/load；路径逃逸、损坏输入、越界和预算失控确定性 fail-fast。
3. 把 HCB basic block 映射为 family-private action sequence，把有序 effect/wait/trace/coverage/snapshot hint 交给 `AstraEmuRuntimeProvider`。
4. 覆盖 148 个 release syscall；任何未实现分支、软失败临时代码或 unknown dispatch 都让 coverage gate blocking，不能返回 `Nil` 隐藏缺失行为。
5. 提交 synthetic fixture 与 sanitized golden；商业样本只生成 ignored local parity report，不进入仓库。

**Current Evidence:** 148 个 release syscall 均有显式 handler，并通过 catalog identity 与 panic-free neutral probe。fixed-step 已补齐 time、timer、输入边沿锁存、bounded VM、VM 后 scene/motion/text/dissolve/wait 更新和状态捕获；snapshot v5 保存 frame index、输入前后态、timer、time、script-visible global state、runtime flags、deferred thread requests 和全部进行中 motion，逐帧 canonical state 使用 generation-cached texture SHA 与有序 associative state，完整 save 以逐 graph 有界压缩保存精确 RGBA。FVP archive 启动只读取 header 和 bounded metadata，entry 通过 ABI v4 range callback 分块读取，不再整包驻留；每次读取进入 session ledger，正式审计另以 4 MiB chunk 读取全部可见资源。ignored 本机授权样本已通过 188 tick/188 frame、输入边沿、snapshot/restore continuation 和流式 OGG 重建，恢复前后 188 帧 CPU RGBA 均与 RFVP 参考逐像素一致；同一签名构建连续两次运行的 visual/state trace 一致。独立审计覆盖 58 个资源、约 8.18 GB，没有 revision/hash 漂移，正常场景只访问 3 个资源、约 22.6 MB，最大单次 range 约 5.0 MB。Headless 默认只落盘 checkpoint，全部帧仍绑定 frame-stream hash。CPU raster 五次 release 运行时间中位数为 14.553 秒，相对 RFVP 13.063 秒为 1.114 倍；时间门禁通过，working set 的五次正式复测与 snapshot 内存门禁仍开放。`Tools/verify_fvp_parity.py` 固定 RFVP 0.5.0 commit，只用于本地 detached worktree 对照；CI 不再联网执行。完整 media parity、正式性能报告和 Windows Manager E3 尚未形成，因此不能标记 `DONE`。

**Linked Test IDs:** `T-S5-FVP-01`

## S5-SIGLUS-01 Siglus 接入门槛

**ID:** `S5-SIGLUS-01`

**Goal:** Siglus 在首批 family 稳定后接入，覆盖 root probe、Scene.pck、Gameexe、`.ss` script、G00/media 和 report policy。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/siglus/implementation-checklist.md`

**Target Paths:** `Emulator/Source/Families/astra-emu-siglus/`、`Emulator/Tests/siglus/`、`scenarios/emu/siglus_full_flow.yaml` planned target

**Steps:**

1. 复用 `LegacyRuntimeProvider` facade 和 failure classification。
2. 实现 Siglus root、Scene.pck、Gameexe header 和授权 material 缺失 diagnostic。
3. 实现 `.ss` header、string table、label、operand decoder 和 basic stack model。
4. 实现 G00/Ogg/OVK/NWA/OMV probe，受保护 stream 只消费用户合法提供的材料。
5. 编写 probe-only report、script fixture 和 full-flow scenario 测试。

**Done Evidence:** Siglus report 不包含 key、payload transform、未授权截图或私有 stream。

**Linked Test IDs:** `T-S5-SIGLUS-01`

## S5-GATE-01 AstraEMU release gate

**ID:** `S5-GATE-01`

**Status:** `IN_PROGRESS`

**Goal:** Release Gate 检查 FVP full-flow、`LegacyRuntimeProvider` facade、显式 runtime/family/UI binding、Slint/WGPU/toolchain/license identity、Minori key-file/streaming identity、ECNU translation policy、filter、host identity 与 report redaction。

**Depends On:** `S5-FAMILY-01`、`S5-AUTOPROBE-01`、`S5-SCRIPT-01`、`S5-TEXT-01`、`S5-FILTER-01`、`S5-FVP-01`、`S5-MANAGER-UI-01`

**Target Paths:** `Engine/Source/Developer/astra-release/src/emu.rs`、`Emulator/Source/Manager/astra-emu-manager-core/src/evidence.rs`、`Emulator/Source/Programs/astra-emu-evidence/`

**Steps:**

1. 增加 explicit runtime/family/UI binding、Slint/wgpu/toolchain/license identity、FVP full-flow/syscall/parity、Minori key-file/streaming 与 translation consent/provider checks。
2. 校验 plugin ABI/engine/rustc/feature fingerprint、binary hash、package eligibility、官方签名、Android APK/native manifest 或 iOS static registration binding。
3. 校验 Windows/Android run identity 绑定同一 build/profile/package/session/input sequence，以及视觉、音频、输入消费、route/terminal 和 surface lifecycle evidence。
4. 所有 report 只允许 alias/hash/offset/size/count/diagnostic；绝对路径、URI、商业 payload、secret、未授权截图/音频或访问控制规避材料必须 blocking。
5. 编写 missing/conflicting provider、missing syscall、signature mismatch、非法 key file、translation consent 和 payload redaction 失败测试。

**Current Evidence:** release gate 已删除 AstraEMU Luau evidence 与 cache identity 输入；`astra-emu-evidence` 会在写入 package sections 前拒绝 unknown field、payload-like field、绝对路径、identity drift 和不完整 E2/E3 lifecycle。当前 key-file/streaming identity 的真实平台 evidence 尚未生成，不能把 passing fixture 当作发布证据。

**Linked Test IDs:** `T-S5-GATE-01`

## S5-PROGRAM-TARGET-01 AstraEMU Manager 与 CLI Program target

**ID:** `S5-PROGRAM-TARGET-01`

**Goal:** AstraEMU Manager 与 `astra-emu-cli` 以 `Program` target 运行；被启动的 case 通过 `AstraEmuRuntimeProvider` 作为 `Game` runtime session 运行，family plugin 仍通过 `LegacyRuntimeProvider` 注册，不升级成独立 Game target。CLI native path 绕过 Manager/Slint，使用 Windows platform host 提供 overlay-free 核心视觉验收；Headless path 复用 `astra-platform-headless` 与物理输入协议。

**Depends On:** `S1-TARGET-01`、`S5-MANAGER-01`、`S5-FAMILY-01`

**Target Paths:** `Emulator/Source/Manager/astra-emu-manager/src/main.rs`、`Emulator/Source/Manager/astra-emu-manager/Cargo.toml`、`Emulator/Source/Programs/astra-emu-cli/`、`Emulator/Platforms/`

**Steps:**

1. 定义 `astra-emu-manager` Target，kind 为 `program`，绑定 desktop platforms。
2. Manager 启动时校验 Program target 和 platform capability。
3. `AstraEmuRuntimeProvider` 的 case target 与 Manager Program target 分开校验。
4. family plugin descriptor 只进入 plugin registry，不写成独立 Target。
5. 编写 Manager target validation、case runtime provider handoff、family plugin isolation 和 local case report 测试。
6. `astra-emu-cli run` 直接创建 provider/session/window/surface，按舞台宽高比路由物理输入；默认静音，显式启用音频，不得隐式启动 Manager 或退回 Headless。

**Done Evidence:** Manager report 包含 Program target id，family report 仍只记录 provider id 和 session id。

**Linked Test IDs:** `T-S5-PROGRAM-TARGET-01`

## 2026-08-04 RFVP stream and ABI identity update

The current Family ABI hard cut is v7. The FVP/Minori runtime snapshot
sections reject v5/v6 and use v7 schemas. Windows PlatformHost now owns the
Media Foundation incremental video/audio sessions, including sequence and
budget validation, bounded prefetch, stable EOS diagnostics, and cleanup on
stop/error/shutdown. Native CLI WGPU playback consumes lazy video frames from
that service; Manager already uses the same service for video and movie PCM.
WMF hardware transforms are requested, but the public boundary remains CPU
BGRA/i16 followed by the required WGPU/device transfer. This implementation
slice is still `IN_PROGRESS`: no clean Release ten-minute mixed-run or formal
Windows E3 parity claim is made here.
2026-08-25 后续（已由 2026-08-26 媒体迁移替换）：Minori AVI 的 64 MiB 预览输入上限、16,384 边长和 64 MiB RGBA 边界现由 AstraMedia FFmpeg adapter 在 demux/decode 前后执行；越界统一阻断，不进入 FVP 或平台 codec。定向 AVI 测试已通过，但这只是安全边界收紧，不改变完整路线、movie gallery parity、正式音频听审或 Windows E3 的开放状态。
同一增量还关闭了未知 AVI stream type：Minori 只接受已验证的 WMV3 video 与 16-bit PCM audio，其他 `AviStreamFormat` 直接阻断；Linux FUSE EOF 读取返回空数据而不是 `EIO`。Windows 上的 support build 通过，Linux target check 受交叉编译环境缺少 GLib sysroot 阻断，真实 Linux mount evidence 仍未形成。
2026-08-26：CLI checkpoint 捕获前新增 pending retained scene / prepared CPU layer 的显式物化，解决 `frame_sample_interval=60` 下首个 checkpoint 没有 submitted surface 的真实边界。当前签名 v9 短程通过 431 fixed steps、7 frames、27 条输入和零 diagnostic；该修复不改变 Stage 5 对完整路线、自然 unlock、正式 audio review、Linux FUSE、macOS extract、FVP v9 和 Windows E3 的 blocking 状态。

2026-08-28：修正 Minori Control/Auto 活动消息等待的受限 modality rebinding。Manager Core 复用已有 `AwaitTokenId`，host 同步替换 pending `Input`/`Time` condition；同类重复和未知 wait 仍 fail fast。代码与定向回归已通过，真实 Sandbox 复测和 Stage 5 的完整路线、正式媒体/音频审查、Linux FUSE、macOS extract、FVP v9、Windows E3 仍保持开放。

2026-08-30：v11 修正了旧实现把右键直接映射到 Save 的语义错误。Minori 现在发布已观察到的 title/gameplay 菜单层级；Host 不解释 item id，只显示并回送结果。Manager 和 Headless 在交互期间暂缓底层 gameplay await。当前完成的是定向回归，不继承旧 ABI 的 Sandbox 现场证据；完整路线、自然 unlock、正式音频 review、Linux FUSE、macOS extract 与 Windows E3 仍保持 blocking。

2026-08-30 后续：开发签名 Release v24 通过序列化 secondary-pointer、方向键、Enter 和 Escape 完成 Save、Load、gameplay Config 三页的打开与关闭。报告通过 138 fixed steps、82 条物理输入、9 个呈现帧、8 个 checkpoint 和零 diagnostic；全部 checkpoint 已查看，页面前后的 gameplay 帧字节一致。Load 返回与 Config 关闭的 ownership 根因已修复并有定向回归。通用退出确认、Release Sandbox、120 Hz 性能和 Windows E3 继续 blocking。

2026-08-31 当前 Release Headless smoke：开发签名 Release 候选在显式 WMF provider
下消费 33 条物理输入，完成 4008 fixed steps、491 个提交/栅格帧和 2713600 个音频帧，
报告为 `passed` 且 diagnostic 为空。首个黑场、日文消息和背景 checkpoint 已人工查看，
未发现明显布局或图层问题。该证据只覆盖有限片段的当前 Family ABI/Host 接线，不提升
完整四路线、Save/restore、正式音频、Release Sandbox 或 Windows E3，Stage 5 继续保持
`IN_PROGRESS`。

2026-08-31 长片段 Release Headless slice：校正 checkpoint 顺序后的当前输入在显式 WMF
provider 下完成 193 条物理输入、24048 fixed steps、238 个提交/栅格帧和 16757248 个
音频帧；48 个 checkpoint 全部通过且 diagnostic 为空。序列以显式 shutdown 结束，未
伪造 terminal；早、中、晚代表性画面已人工检查。该证据仅覆盖 Headless 长片段，不提升
四路线自然解锁、Save/restore、原版同点视觉、正式音频、Release Sandbox 或 Windows E3，
Stage 5 继续保持 `IN_PROGRESS`。
2026 年 9 月 1 日 Save/Load 卡片对齐已进入 provider 主路径：v3 save envelope
保存本地时间、注释和 96x54 gameplay 缩略图，列表与 Load 均执行严格 metadata
校验；Host-owned Layer2D 按原版槽位置叠加缩略图，Save/Load 底部按钮和页标签
使用已验证资源。Minori library 194/194 通过，仍只计 E1/E2 provider/Host 证据；
真实原版同点视觉 parity、四路线、Release Sandbox 和 Windows E3 继续阻断 Stage 5
完成。
同日页眉复核进一步固定标题图 `(64,16)` 与页标签图 `(608,16)` 的舞台坐标，并新增
provider draw-origin regression。原版标题 Load 与剧情 Save/Load 的 `Auto Save`/
`page #01` 上下文差异已进入 VM（Page0 与全局槽 20/Page2）；Auto/Quick/manual 槽
ID 的持久化规则仍未确认。Save 页 `Back` 裁剪已统一到所有 page index，Save/Load
parity 继续保持开放。

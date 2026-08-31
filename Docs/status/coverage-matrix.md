# Implementation Coverage Matrix

2026 年 8 月 31 日 Minori 原版 locale coverage：mount options v3 要求日文原版变体、
严格 CP932 locale hook 和非符号链接 `perseus.exe`；PAZ/ANI/.sc 的字节边界均拒绝
malformed input，message publisher 不再调用 translation Hook。中文 `.mys` 与本地化
exe 仍仅是脱敏 inventory 事实，未被加入 source resolution。187 项 Minori library
定向测试通过；该覆盖不代表完整路线、原版同点视觉、Release Sandbox 或 Windows E3。

2026 年 9 月 1 日按同一日文原版 profile 重跑脚本 census：89 个 `.sc`、33728 行、
33695 条 command、29 个已观察 opcode，unknown opcode 为 0；其中 `chain` 55、
`if` 20、`goto` 10、`select` 2、`movie` 15、`end` 85。该项同时回归 `.include`
目标的严格 CP932 解码与 ASCII `.sc` URI 校验，属于 E1/E2 解析证据，不关闭路线或
视觉验收。

2026 年 8 月 31 日标题页 Exit 回归：provider 定向测试确认标题页 Exit 直接进入 terminal，
且没有向 Host 发布 confirmation；剧情页 `game_exit`/`game_return_title` 继续使用 Family ABI
confirmation。该项是 E1 provider 行为证据，不提升 Release Sandbox、完整路线或 Windows E3。

2026 年 8 月 31 日确认框覆盖：原版剧情 Game→Exit/Return title 的正文使用 ASCII `?`，
标题页 Exit 不弹确认；Minori provider 的 Family ABI transaction 已同步文案，取消与
接受仍分别恢复 wait 或进入返回标题/terminal。定向 provider 与 Host 结果校验通过；这
不是完整路线、Release Sandbox 或 Windows E3 证据。

2026 年 8 月 31 日原版禁用菜单项导航覆盖：授权 Windows Sandbox 观察到，右键菜单的
方向键焦点会经过灰色禁用项、跳过分隔线，鼠标点击禁用项不会提交 command；Enter
在禁用项上关闭弹出菜单，Space/Right 保持菜单活动。submenu 返回时焦点恢复到父项。
CLI Headless 与 Manager Host 现按 ABI `order` 保留非分隔线禁用项，并区分这些激活
结果；对应定向回归通过。该项只关闭 Host 导航语义差异，不代表真实 Manager 窗口、
Release Sandbox 视觉验收或 Windows E3 已完成。

2026 年 8 月 31 日 Manager Host 菜单导航覆盖：Slint 宿主只渲染 Family transaction
当前父节点的启用同级项；方向键在该集合中循环，Right/Enter/Space 进入 submenu 或
选择 command，Left/Escape 返回或关闭根菜单。焦点输入不会进入 gameplay，submenu
不会被伪装成 command。适配器、Manager、Family 菜单校验和 Windows native anchor
定向回归通过；真实 Manager 窗口焦点/定位截图、Linux 原生菜单、Release Sandbox
视觉验收和 Windows E3 仍保持 blocking。

2026 年 8 月 31 日原始尺寸 Host 回执覆盖：Windows/macOS Host 应用
`RestoreOriginalSize` 后，Minori 在下一固定 step 清除并持久化 Family-owned
fullscreen 状态；菜单重开重新包含全屏项。新增 provider 回归覆盖切换全屏、原始
尺寸和重开菜单，Host `Rejected`/`Unsupported` 仍保持 blocking。该覆盖只修复
平台窗口状态与 Family transaction 的一致性，不提升 Release Sandbox、完整路线
或 Windows E3 证据等级。

2026 年 8 月 31 日 Family ABI v14 text-input coverage：Minori Save 的 Comment
提示改为 Host-owned `LegacyTextInputTransactionV1`。Windows 使用 DPI-aware
owner-modal Win32 编辑框，Host 只返回一次有界结果；Minori 保存格式负责持久化
注释，Host 不写入日志或 evidence。macOS/Linux/Web/Android/Headless 当前明确返回
`PlatformNotImplemented`，等待各自原生应用 UI 或 typed Headless driver；不使用
Slint overlay 或隐式 fallback。Family API/FFI、Manager、Minori 与平台校验测试
通过。本项是 E1/E2 接线证据，不是 Save 页面视觉 parity、Release Sandbox 或
Windows E3 证据。

2026 年 8 月 31 日 Windows confirmation geometry coverage：Host 原生 presenter 以
96 DPI 下观察到的紧凑 350×164 为基准，按 owner window 有效 DPI 缩放窗口、消息、
按钮；无 owner 的 service 调用使用系统 DPI，并放置标准 Win32 question icon；checked
arithmetic 的正常与饱和边界测试通过。
Family ABI transaction、`是(Y)`/`否(N)` 助记键和 owner-modal 生命周期没有变化。
该项是平台呈现 E1/E2 覆盖，不是原版同点截图、Release Sandbox 或 Windows E3 证据。

2026 年 8 月 31 日 Release Sandbox 启动复核：Windows desktop builder 强制
`+crt-static`，新的开发签名包已在同一 Sandbox 启动 AstraEMU Manager，VC runtime
依赖阻断已关闭。该证据只覆盖包启动，不代表 Minori 路线、Headless E2、视觉/音频
审查或 Windows E3 通过。

2026 年 8 月 31 日 Window 菜单状态覆盖：原版窗口化时显示全屏切换、原始尺寸、
禁用的高精度尺寸変更和抗锯齿四项；全屏时隐藏全屏切换，恢复原始尺寸后再显示。
Minori family 按该状态发布三项/四项 transaction，并在执行前校验活动 transaction
中的 item、启用状态和命令类型。定向菜单与命令测试通过；Linux 原生菜单、Headless
GPU E2、Release Sandbox 视觉验收和 Windows E3 仍保持开放。

2026 年 8 月 31 日 native confirmation caption coverage：Linux Host 在存在 live
owner window 时读取 owner caption，没有 owner 时保留 Family transaction title；该规则
与 Windows/macOS presenter 保持一致。此项只属于 Host 接线覆盖，Linux context menu、窗口
命令和实际桌面 evidence 仍保持开放。

2026 年 8 月 31 日 Windows Manager confirmation coverage：Manager 的无窗口
audio/decode service host 已接入 Family ABI v14 `ShowConfirmation`，在同一 service
thread 使用 Host-owned Win32 modal window，并保留 `是(Y)`/`否(N)` typed label；不
创建第二个 Winit loop，也不把菜单、窗口命令、帮助、About 或 surface 偷渡到 service
host。移除 `rfd` `common-controls-v6` 后，CLI loader boundary 和平台 2/2 回归、
Manager build 均通过，确认结果不会因 service host 缺少 game window 而永久 pending。
该覆盖属于 E1/E2 接线证据，不是原生窗口视觉、Release Sandbox、正式音频或 Windows
E3 证据。

2026 年 8 月 31 日路线输入 smoke：现行 `runtime.input_or_terminal` 序列在开发签名
Release CLI 上消费 195 条物理输入，完成 24048 fixed steps、48 个 checkpoint、109
个提交/栅格帧，diagnostic 为空。该运行使用显式 WMF 与稀疏 frame sampling，尾部
主动 shutdown，不声明 terminal、自然解锁、120 Hz GPU E2、Release Sandbox 或
Windows E3；四路线、同点视觉、正式音频和 save/restore required checkpoint 仍为
blocking。

2026 年 8 月 31 日确认框覆盖：原版 Windows Sandbox 的退出与返回标题确认框
文案、按钮顺序和取消后的 wait 保持已记录，Minori 通过 Family ABI v14 发布，
平台 Host 负责 native 呈现；Headless 不伪造 native dialog，只消费物理方向键、
Enter/Space/Escape。旧路线输入中的 `runtime.awaiting_input` 观察键已经被 hard
cut 拒绝，当前尚未以新序列完成完整 Release Sandbox 或 Windows E3，因此本项
仍为 E1/E2 行为对齐，不能标成正式通过。

同轮的菜单窗口动作也通过 v14 typed system-command channel 传递。Windows/macOS
Host 只在显式窗口绑定存在时执行原生全屏、原始尺寸恢复、缩放采样和帮助动作；
Manager、Headless 和无窗口 CLI 对未绑定能力回送 `Unsupported`，不解释 item id、
不伪造成功，也不留下 pending command。Minori 对 Host `Applied` 的 host-owned 操作
只释放挂起事务，不把窗口或外部进程状态写入 VM。About 图像、Linux GTK 原生菜单、
帮助/浏览器实际启动和完整路线仍是未完成的正式验收项。

2026 年 8 月 30 日 Family ABI v12 确认覆盖（历史记录）：Minori 的 `game_exit`、`game_return_title` 与 Host `window.close` 由 Family 发布有界 confirmation transaction，平台 Host 以 native confirmation provider 呈现，后续固定 step 回送 `Accepted` 或 `Cancelled`。取消保持底层 wait，接受才执行 terminal 或返回标题；重复、过期、错配和确认期间的 gameplay input 继续阻断。Manager、Release CLI、Headless 和 Minori 的定向回归通过；该结果属于当时身份的 E1/E2 接线证据，当前 v14、Release Sandbox、120 Hz 和 Windows E3 仍开放。

2026 年 8 月 30 日 Minori 原生菜单 Auto 覆盖：Manager Core 现持有唯一的受限 wait 重绑规则，RuntimeWorld adapter、Manager 和 Release CLI Headless 共同只允许 `Input↔Time` 与 `Time→Time`；同批重复和其他类型继续阻断。开发签名 Release v21 通过 secondary-pointer 与方向键选择 Auto，再次选择后恢复 Normal。报告通过 371 fixed steps、36 条物理输入、9 个呈现帧、6 个 checkpoint且零 diagnostic；Auto 开启后的画面发生预期推进，关闭后继续运行 2 秒保持不变。保留画面已人工检查。该覆盖属于原生菜单 Auto 定向 E2，不关闭完整路线、Release Sandbox、120 Hz 性能门禁或 Windows E3。

2026 年 8 月 30 日 Minori 原生菜单 Skip 与 Control 覆盖：当前开发签名 Release CLI 用 secondary-pointer 打开 v11 family menu，并以物理方向键选择 Skip。定向报告通过 674 fixed steps、41 条物理输入和 4 个 checkpoint，未读消息在选择前、选择后及 10 秒后画面字节一致。受 pragma 门控的 Control 已加入对应 Host-owned message wait，避免发布同一 token 的替换 wait；173 项 Minori library tests 通过。独立空白进度的 Control 首路线随后通过 15636 fixed steps、28 条物理输入、251 个呈现帧和 3 个 checkpoint，结局影片自然完成，路线返回标题并退出，自然解锁数为 1，diagnostic 为空。三张保留画面已人工检查。该覆盖关闭当前身份的未读 Skip 定向 E2 和 Control 首路线 E2，不替代 Release Sandbox、120 Hz 性能门禁或 Windows E3。

2026 年 8 月 30 日 Minori Skip 行为覆盖：原版空白进度现场确认持久 Skip 不推进未读消息，Auto/Skip 原生菜单项也不显示勾选。runtime 现以已记录的 message read identity 约束持久 Skip；Control 快进继续使用独立 pragma gate。新增 unread/Control 回归，Minori 173 项 library tests 通过。该覆盖属于 runtime/menu E1，不提升完整路线、Sandbox 或 Windows E3。

2026 年 8 月 30 日 WMF composition 增量：AstraEMU CLI 和 Manager 现可显式选择 `wmf` 或 `ffmpeg-vcpkg`，Minori 拒绝 `disabled`、未知值和运行时 provider 切换。Windows 桌面包默认 `wmf`；构建器仅在明确选择 FFmpeg 时启用其 feature，并把选定 provider 写入脱敏 package evidence。默认与 FFmpeg feature graph 的定向编译均通过。尚未运行签名 Release WMF movie checkpoint，因此画面方向、padding crop、音频时序、fence 和原版同点 parity 仍开放。

2026 年 8 月 30 日原版影片后端复核：原版二进制明确创建 DirectShow `CLSID_FilterGraph`、`IGraphBuilder`/`IFilterGraph2` 与 windowless VMR7，关键 COM 调用失败会进入清理路径，未发现静默切换解码后端。AstraMedia 新增有界只读 COM `IStream` adapter 和显式 `astra.decode.wmf.incremental` registry provider；公开 MP4 已覆盖统一双轨 packet、seek generation 与 cancel。授权 AVI 通过统一 provider 到 EOS，共 2106 个单调 PTS 视频 packet 和 2110 个 PCM packet，未复制 encoded source 到 HGLOBAL 或 plaintext spool；固定 FFmpeg 路径仍对 7 个 concealment frame fail-fast。当前关闭公共 provider seam，不等于 Minori 生产接线、原版逐帧 parity 或 Windows E3。

2026 年 8 月 30 日自然鉴赏子页：复用同一隔离 writable identity，不注入解锁状态，序列化物理输入进入 `Memories`、BGM、CG、回想和影片列表。报告通过 82 fixed steps、20 个呈现帧、9 个 checkpoint、45 个资源且零 diagnostic；九张画面已检查，未见明显缺字、裁剪、拉伸、错层或残影。该证据关闭当前自然 progress 到鉴赏子页的 Headless E2 输入/呈现链路，不关闭影片实际播放、原版像素 parity、四份独立绿色路线报告或 Windows E3。原版 Sandbox session 因原程序重复异常对话框无法用于本轮同点对照。

2026 年 8 月 30 日 FFmpeg 依赖复核：根 vcpkg manifest 与 Windows CI 现固定 FFmpeg `8.1.2#3`，AstraMedia provider 严格校验 `libavcodec 62.28.102`。定向版本测试与 10 个增量流测试通过。授权 WMV3 在普通文件输入、外部 FFmpeg 和 custom AVIO 下均暴露 7 个损坏帧，说明 custom AVIO 不是唯一根因。AstraMedia 现读取 `decode_error_flags` 并以 `ASTRA_FFMPEG_CORRUPT_FRAME` 阻断，不再让 concealment 只出现在 stderr。影片 checkpoint、原版同点视觉比较和正式 E3 仍保持 blocking。

2026 年 8 月 30 日自然解锁链：在隔离 launch/writable identity 中从零顺序执行四条真实路线。Sui 通过并观察累计解锁 1；Ren 已完成 route 和结局影片，但报告因过时的累计值断言失败；Ayame 随后通过并观察累计值 3，证明 Ren 的持久化被下一 session 读取；Tohka 通过并观察 `route_complete` 与累计值 4。新标题 session 的 15 fixed step 报告通过，物理输入可进入自然出现的 `Memories`，标题和鉴赏根页两个 checkpoint 已检查且无明显视觉阻断。该证据把自然 clear 写入、跨 session 读取和标题 gate 提升到真实 Headless E2 链路，但四份独立 route report 尚未全部通过，鉴赏子页和原版视觉 parity 仍开放。未跳过的两段 WMV3 均提交 completion，FFmpeg concealment 使逐帧质量继续 blocking。

2026 年 8 月 30 日消息控制回归：Minori runtime state 已硬切到 v28。IDA 确认的 `\\a`、`\\v` 与 `MsgSubCmd load` 已进入 typed parser；控制标记不会成为可见正文。voice wait 通过 AstraMedia/Symphonia 的 seekable metadata reader 读取 revision-pinned VFS stream，避免无缓存压缩包上的整文件物化。inline load 按 fixed clock 保存 pending state，并以 current/next retained texture 执行互补 alpha 交叉淡化。授权样本的定向 Ogg 探针耗时 12 ms。真实标题启动回归完成 5258 fixed steps、83 个采样帧和三个 checkpoint，diagnostic 为空，最大 `runtime_step` 为 0.553 秒；保留帧的模型检查未见新增裁剪、拉伸或图层残影。该结果属于单路线 Headless E2 回归，不关闭罕见行精确 checkpoint、原版视觉对照、其余三路线 E2 或 Windows E3。

2026 年 8 月 30 日流式分配回归：Minori decrypt chunk 改为消费 source-owned `Vec<u8>`，通用 Blowfish 与后续 RC4 在同一 allocation 原地执行；`MinoriEntryStream` 截断后直接保留该 buffer。定向测试覆盖 buffer identity、跨 chunk zlib checksum、multipart、movie transform、随机 range 重开与无明文 cache。它只关闭 chunk 内重复 allocation，不把合成回归升级为真实峰值内存规模证据。

2026 年 8 月 30 日 Release 路线复核：分支已 rebase 到当前 `origin/master`，官方 Minori 桌面包现在强制同时编译 Manager/CLI 的 `ffmpeg-vcpkg` binding。严格补齐 ASCII casefold VFS lookup、已观察的尾随空字段、primary Firefly fadeout，以及全包唯一的 `.panel 1 * <resource>` 形态后，当前开发签名 Release 包完成 5212 fixed steps、4382 个呈现帧、17 条物理输入和 4120576 个非静音音频帧，`route_complete`、返回标题、解锁计数 4 与零 diagnostic 成立。三个 checkpoint 已实际查看。该通过报告开始时平台进度已含四个 clear flag；三到四的自然写入另一次运行因测试尾段错误没有形成 passed report。未跳过的 WMV3 全流还出现 FFmpeg concealment 输出，故四路线自然解锁、影片质量、正式音频 review、save/restore checkpoint、Release Sandbox 和 Windows E3 仍为 blocking。

2026 年 8 月 29 日 Minori key-file/streaming hard cut：现行 VFS 不再执行 AstraEMU Luau patch，也不建立明文 cache。旧 cache second-run 与 aggregate hash 仅保留为迁移历史，不能证明新 identity。当前已完成严格 key parser、有界私有文件、流式 PAZ reader、旧命令/schema 删除和受影响 crate 回归；当前 identity 的真实八包 full verify 已覆盖 14502 entries 和 6624958365 decoded bytes。FFmpeg 增量入口已改为 custom AVIO，120 Hz GPU Headless 完成首段真实影片全流解码与 fence；诊断路线没有形成最终 artifact 或 route-pass report，因此四路线 Headless GPU E2 与 Release CLI Sandbox 验收仍开放。

The signed Manager also reached its runtime-active window in the authorized Windows Sandbox with no default audio device; Diagnostics showed no blocking diagnostic after `NullAudioLane` selection and exposes `audio_endpoint=null` for the session. This is startup/UI evidence only. The null endpoint is excluded from physical-audio evidence and the Sandbox did not yield a writable artifact, so Windows E3 and formal audio review remain open.

Current AstraEMU contract identity is Family ABI v14 (`astra.emu.family_abi.v14`), a hard cut from v13. The ABI carries typed `Open`/`Select`/`Dismiss` requests, bounded menu, confirmation, and text-input Host ports, and typed system-command transaction/result pairs. Minori publishes the observed title/gameplay hierarchy, confirmation semantics, and Save comment prompt; platform Hosts present them and return typed results without interpreting family item ids, confirmation text, or command payloads. Duplicate, stale, ambiguous, active-choice, active-media, confirmation-input, and text-input conflicts remain blocking diagnostics.

2026-08-28 runtime follow-up: missing native audio output (`ProviderUnavailable`) now uses the shared bounded `NullAudioLane` inside `FamilyAudioService`, preserving the Kira/resampling/telemetry path without claiming physical audio. Manager emits an explicit `audio_null_device` marker and keeps that sink out of the physical `audio_non_silent` evidence bit. Minori message waits now expose every directly consumed activation edge, including `pointer.primary`; the targeted provider/Manager regressions and a Sandbox click-then-confirm run pass without duplicate-ready waits. This is startup and wait-contract evidence only; full route, formal audio review and Windows E3 remain open.
The null sink now validates the exact stereo chunk shape and finite samples at both capacity and submit boundaries; malformed chunks fail before telemetry advances. This is a local contract regression and does not upgrade null-device runs to physical-audio coverage.
The public `FamilyAudioService` lifecycle is also covered when `OpenAudioOutput` returns `ProviderUnavailable`: worker startup, a queued suspend command and clean shutdown complete with `null_device=true`. This closes the no-device lifecycle path only; physical audio and Windows E3 remain open.

2026-08-28 startup ordering hardening: `FamilyAudioService::start_with_client` now waits for an explicit worker endpoint-selection handshake. A missing device is therefore resolved to the bounded null lane before the service is exposed; any other output-open failure is returned synchronously and the owned host is cleaned up. Focused support/Minori/Manager tests pass. This removes an asynchronous startup race but does not create physical-audio evidence.
`has_physical_audible_output()` is now the shared evidence predicate; it deliberately differs from mixer-level `has_audible_output()` while a null endpoint is active, and Manager uses the physical-only API.

2026-08-28 global-progress lifecycle evidence: two independent Minori provider sessions share the explicitly bound writable-file port; the first persists `REN_CLEAR`, and the second loads it before evaluating its entry script, reports one unlock, and leaves `SUI_CLEAR` unset. This closes the provider/VFS load-order regression at E1 only; natural four-route unlock, full gallery and Windows E3 remain open.

2026-08-28 Control/Auto follow-up: an active Minori message may rebind its existing family wait token between `Input` and `Time`. Manager Core reuses the existing runtime await identity and Manager host replaces only the pending condition; same-kind, non-message and batch-duplicate tokens remain blocking. This closes the observed duplicate-token crash path only and does not change AstraEMU `IN_PROGRESS` or Windows E3 status.

2026-08-29 hard cut：此前 private-profile/cache identity 的八包 second-run 只保留为历史。当前 key-file/streaming reader 已重新完成八包 full verify；重复密文读取有合成回归，峰值内存规模验证仍保持开放。

2026 年 8 月 27 日增量媒体复核：`astra-media::IncrementalMediaPlayback` 已把播放配置、单调 tick、轨道/packet 形状、视频 lead/lag、迟到策略和音频/视频 packet 预算收进公共游标；`dropped_video_packets` 只在显式 `Drop` 策略下增加。当前签名 FFmpeg Minori slice 以 3102 个 fixed step、9 个 retained frame sample、连续 movie stop/completion 和标题观察完成，报告为 `passed` 且无诊断。该结果仍是 Headless E2 provider/media 证据，不关闭完整路线、正式音频听审、gallery/cache second-run、Linux FUSE、macOS extract、Manager 实机预览或 Windows E3。

同日首路线重验：按当前 v9 typed observation 重新生成物理输入后，签名 release plugin 通过同一 FFmpeg incremental provider 完成首路线。报告为 `passed`，3,034,309 fixed steps、16,150 条物理输入、53 个 retained frame sample、31 个 checkpoint，route terminal、`route_complete`、自然 unlock count=1 和最终 Exit 均成立，diagnostic 为空。该输入没有把已删除的 observation hash 当作成功条件，也没有把过时的首 choice 等待点计入本次 checkpoint；choice 语义仍由独立真实 slice 覆盖。因此这项证据关闭当前首路线的 FFmpeg/media/terminal 自动路径，但不关闭四条自然路线、第四条路线后的完整 Memories、正式 WAV 听审、cache second-run、Linux FUSE、macOS extract、Manager 实机预览或 Windows E3。

2026-08-26 media binding update: Minori AVI preview and incremental playback now use the shared AstraMedia `ffmpeg-vcpkg` provider through an explicit registry binding. The handwritten WMV3/AVI production path has been removed; missing or mismatched FFmpeg binding is blocking. A real authorized sample slice completed 60 fixed ticks with 16 presented frames, 111104 decoded audio frames, non-silent audio and zero diagnostics. After the ABI-v9 rebase, the same sample was rerun at 139 fixed ticks with five retained samples, 638 bounded VFS reads and zero diagnostics; current `title_initial`, `config` and `movie_60` artifacts were manually inspected. A subsequent release rerun with the retained Layer2D composite cache kept the same reads, samples and diagnostics while reducing total step time from about 81.9 s to 35.9 s and effect dispatch from about 48.7 s to 22.4 s; this remains a performance diagnostic, not a formal gate. This is Headless E2 media evidence only; full route, formal performance, Manager window preview and Windows E3 remain open.

Manager and Headless CLI family-mounted PNG/JPEG/BMP/WebP previews now use an explicit `astra.decode.image` registry binding and bounded RGBA8 handoff. Minori ANI/SQZ previews use the explicit family-owned `astra.decode.minori.image` first-frame binding in both consumers, and family audio previews use explicit Symphonia metadata-only handoff; animation playback and unbound video remain blocking/open.

Manager startup uses a pure-Rust Minori idle provider and loads FVP only from an explicit family selection; the selected family is rebuilt with its validated VFS binding before launch. This is focused composition-root evidence, not Windows E3.

Minori `progress_in_background` is now exposed as a bounded provider observation and consumed by the Windows native host for Minori-only focus suspend/resume. Focused provider/CLI evidence passes; real focus/audio and Windows E3 evidence remain open.

Current AstraEMU identity note: the active contract is Family ABI v14
(`astra.emu.family_abi.v14`) with Product Runtime Provider ABI v4. Minori uses
`Native + MultiLayer`, Host-owned surfaces, synchronous Hook, typed
`LegacyFilterGraphV9`, typed system-menu/confirmation/system-command/text-input transactions and
writable-file ports. CLI、Manager 和 Minori 的增量 consumer 已恢复编译，Manager
typed filter graph 已走 WGPU，旧 Scene2D transaction consumer 已删除。Minori
签名真实样本已通过一轮 Headless lifecycle，但 checkpoint 阶段标签与完整路线
视觉门禁尚未闭合。ABI v8/v9/v11 E2 只作历史回归基线；本行保持 `IN_PROGRESS`。

1999 原版补丁器已接入 workspace。公开测试覆盖 edition fingerprint、RIFX 资源图边界、唯一 CASt binding、script ID 大端读写、ProjectorRays hash/timeout、完整目录复制、原子清理、manifest 和发布包 hash。私有 `inspect → apply → verify` 已证明原安装目录保持只读，成品保留 `DATA/MENU.dxr` 原名，其余原文件逐项保持 hash。受控 launcher 已用 Locale Emulator Core 的 CP932/LCID `0x0411` 环境成功创建 32 位 projector，并把 Director 7 残留的 1 像素 outer frame 删除；实测 outer/client 同为 800×600，window style 为 borderless popup。标题第三按钮的完整视觉状态与路线跳转仍需形成同一轮 E3 报告，当前不计入 AstraVN Player 的 Windows E3 coverage。

TsuiNoSora 当前覆盖边界：严格 ProjectorRays codec、2527/2527 binary resource conversion、完整 typed story IR、私有 NativeVN project/package、Classic/Modern Yakui UI 和 Player secondary-input action routing 已落地。2026-07-20 的私有 full coverage report 记录 578 个 source resource、724 个 handler、27,042 条 command、37 条自动化路线、1,328 个 choice、3,092 条 media command 与 17,161 个 wait command，状态为 `pass`。private RC package 保留全部转换内容，但只承诺 `Y` 可玩；其余 36 条路线属于 present-but-unvalidated research 内容。v13 预检 package identity 为 `sha256:d7c81ec4f4494820d8410b2f88927dad31dbb5eb0ade3c03ec04090c1c796ea0`；最终 build identity 要在截图凭据和文档冻结后重建。视觉矩阵在 `wgpu_offscreen`、Windows DX12 离散 GPU 上通过 43 个 checkpoint；独立的 Y 路线 E2 用 445 条序列化物理输入和 27 次选择抵达 K movie 首个权威 wait，证明 Y 的完整边界。

RC 的 13 项 reference 已完成像素预检，全部满足各自固定门禁；`006` 仍是唯一允许绑定具名 `astra.headless_tolerance_approval.v2` 的色彩容差项。UI010 至 UI014 的系统窗几何偏差为 0 px，UI009 的选择菱形列偏差为 1 px。模型已查看全部五联图；30 张输入和 12 组稳定捕获契约均已闭合，权威 manifest 与 node map 已同步。Director movie 入口现按 Score snapshot 恢复初始可见 layer；source-bound package crypto、不透明授权目录、CLI build/bundle 与 Player bootstrap 已形成 contract/E2。商业明文、媒体签名和私有路径扫描均通过预检；最终同身份重跑和 formal signoff 尚未闭合。Windows E3 显式延期，不作为本轮 RC 门禁，状态保持 `IN_PROGRESS`。旧 synthetic story 与旧 worktree 证据不计入当前 coverage。

AstraEMU 当前实现边界以 Family ABI v14 为准：Host-owned surface、retained `Layer2D`、同步 opaque Hook、UTF-8 translation companion、安全相对路径 writable-file、typed filter graph、双向 typed system-menu、one-shot confirmation、system-command 和 text-input transaction 已进入公共契约。Minori resource/text surface、CLI layer consumer、Manager Hook/Layer2D consumer、物理右键到 family system UI 的映射、退出/返回标题确认以及 Save Comment prompt 已进入 v14 主路径；旧 v13 及更早 ABI 只保留为历史记录，不能继续加载。新的真实样本 slice 已能输出不同画面与非静音音频，但尚未到 terminal，checkpoint 标签也未完全对应目标页面，下表旧 ABI 完整路线只保留为历史记录。

2026 年 8 月 30 日系统页输入所有权增量：Family API 用规范布尔 observation 表达 family UI 是否独占物理输入，CLI 与 Manager 不再解析 Minori 页面名称。官方开发签名 Release 候选通过 Quick Save、推进、Load 页和 restore 的 2047-step Headless E2，6 个 checkpoint、非静音音频和零 diagnostic；恢复帧与保存前一致。视觉复核同时发现一组罕见 message 行末控制标记仍被当作正文，故消息视觉完整性、四路线、影片复核、Sandbox 与 E3 继续开放。

2026 年 8 月 23 日的 consumer 增量已完成动态 loader、CLI 与 Manager 四个 Host port 组合；Minori 先执行同步 translation Hook，再以 CosmicText/Astra Renderer2D 写入文字 surface。Manager per-layer typed graph 通过公共 validator 后由 WGPU 执行，不存在 CPU fallback；旧 Scene2D transaction consumer 已删除。当前签名样本 slice 消费 42 条物理输入，提交 154 帧并输出 134144 个音频 frame，diagnostic 为 0。模型检查确认画面有标题、系统背景、正文与场景变化，也确认首个 checkpoint 是黑色过渡帧、页面标签存在错位，因此仍是受限 E2 诊断，不是完整视觉通过。

2026 年 8 月 25 日脚本引用审计增量：Minori open 可显式接收 `astra.resource_audit=full`，由 Host bounded enumeration 扫描全部 `.sc`，复用 VM grammar 对 stage、character、effect、audio、movie、panel 和 chain 引用做非空/大小/revision 校验。审计只形成脱敏计数与 identity digest；普通 lazy 运行、未知 opcode、VFS enum 缺失和完整路线/人工 review 的证据边界不变。

2026 年 8 月 25 日消息状态增量：runtime schema 已硬切到 `astra.emu.minori.runtime_state.v24`；message completion 记录按脚本 revision、source span、message id、text hash 形成的排序 bounded read identity，并覆盖 snapshot round-trip 与重复/无序 corruption blocker。逐字 reveal 公式和原版未读 Skip 策略仍未知，不计入完整消息行为 coverage。

2026 年 8 月 25 日 Manager VFS preview 增量：文本 preview 先识别 UTF-8/UTF-16 BOM，再对已知脚本/配置扩展名尝试 CP932；包含 NUL 或 malformed input 的内容保持 hex 视图，UI 显示实际编码。Manager 同时接入 Minori family mount、解密 URI range reader 和静态 runtime provider。该项只覆盖文本检测和 family mount，不替代显式 image/audio/video decode binding 或真实 Manager media preview evidence。

同日 backlog audio 增量：`backlog_voice_playback` 关闭时，replay 保留 backlog 中的 voice identity 但不提交新的播放命令；角色 voice filter 与该 preference 均有 VM regression。全屏 Host effect、逐字速度和真实 host 音频听审仍未关闭。

FVP 已进一步移除退役的 snapshot、text lease、session resource 和 step budget consumer，并恢复动态签名 lifecycle 测试。scene/text 仍以明确 migration diagnostic 阻断，因而不能把这一结果计作 presentation、Headless 或产品 E2 coverage。

ABI consumer implementation 基线已更新到 `635527831e89e5ff9b87ac165b5b5532e28356c6`。Filter graph 以 typed node、target 和 parameter 穿过 Family 与 Product 边界；提交 schema 已重新生成，不存在 string/hash graph resolution。RFVP fork 更新为 `23ae395bcc0499f737d63759902308b30ca37800`，已删除退役 VFS decrypt/private-profile/cache identity consumer，并绑定唯一 workspace ABI source。

2026-08-03 的 Windows native 诊断另确认 PlatformHost command queue 未唤醒 Winit 是 present backlog 的直接根因。当前 command submit 和 HTTPS completion 已通过 `EventLoopProxy` 事件驱动，800-step signed Release 复跑无 backlog，scene present 间隔中位数 16.677 ms、WGPU present p99 6.129 ms；该短跑不替代 hover 动画语义、Family ABI scene bulk 零拷贝、10 分钟 audio soak 或 Manager E3。

当前热路径约束也覆盖 EngineCore/VFS：Shipping `RuntimeWorld` 不执行 StateMachine candidate cycle 的 postcard/hash 指纹，只保留有界 microstep 失败边界；禁用 integrity report 使用常量 marker，不在每 tick 计算 hash。`RangeReadResult` 与 Family v7 VFS wire 只携带 revision、range 和 owned bytes，重复读取由 revision/lifecycle 契约约束，不计算 per-read content hash。VN provider 的产品音频已改为 typed live cue，Shipping 热组件改为 owned postcard allocation + disabled marker；VN presentation/view-state/timeline 的实时 persisted DTO 仍是未关闭的独立热点，因此本项仍不能标记为全引擎零序列化完成。

2026-08-07 Windows convergence 验证已通过 workspace fmt、clippy、Headless build、workspace tests、Windows export、Headless lifecycle inventory 与 shipping graph 门禁。Shipping 平台依赖图不再携带 `astra-headless-protocol`；硬件 renderer identity 只在 Headless artifact 边界转换为 Evidence DTO。该结果不包含 10 分钟 Windowed E2 或同身份 Perfetto，因此不提升 AstraEMU 性能 evidence 等级。

2026-08-12 Minori coverage 更新：签名 ABI v8 plugin 在真实八包上以相同物理输入连续完成两次标题启动的路线级 Headless E2。两次均推进 31011 fixed steps、提交并栅格化 31627 帧、消费 16947 条输入、通过 31 个 checkpoint，并覆盖 Config、backlog、用户 save/restore、真实影片、snapshot round-trip、自然 unlock、结局返回标题、最终 Exit 和零 diagnostic；VM、visual、terminal、coverage、scene、raster 与 audio identity 全部一致。独立真实脚本 slice 以严格 observation 捕获首个 choice 与进入实际分支后的 post-choice，choice 帧 hash 命中完整路线 fixed step 13928。模型检查 review bundle 要求的 frame，未发现缺字、裁剪、拉伸、影片比例错误、图层残影或人物生命周期泄漏。Kira limiter 后 output peak 为 0.989551，output overload 与 underflow 均为 0；WAV 自动量测无 full-scale sample。涉及语音的整段试听仍未完成，正式 `validate-review` 因 `ASTRA_HEADLESS_REVIEW_AUDIO_LISTEN_PENDING` 保持 blocking。该证据关闭当前单路线的 Headless 自动确定性、自然解锁和返回标题，不关闭正式模型/人工 review，也不覆盖原版只在第四条 clear route 后开放的 `Memories` 页面、Windows E3 或完整 Minori 产品体验。

2026-08-22 Minori Config 增量：runtime state v23 覆盖 29 类已确认动作、Apply/Cancel draft、pointer hit map、滑块、四类 overlay 资源、WAV 试听和音量/静音映射；114 个 family library tests 通过。真实八包短程 Headless 以 166 fixed steps、171 帧、42 条物理输入和 6 个 checkpoint 验证 screen-effect checkmark、BGM knob 与 `BGMTest.wav` 试听，snapshot round-trip 和自动门禁通过。模型视觉审查通过；完整 WAV 未人工试听，正式 review 保持 blocking。全屏、逐字速度、视觉开关对剧情的实际影响和角色语音筛选仍无行为覆盖。

2026-08-25 Minori Config 持久化增量：在显式 `astra.writable_file.v1` storage binding 下，已应用配置使用 `astra.emu.minori.config.v1` bounded postcard envelope 保存，严格绑定 case/package/profile identity，写入采用 temporary file + atomic replace。缺失文件使用默认值；损坏、越界、矛盾 stat/read 或 identity drift 都返回 blocking diagnostic。gameplay save slot restore 保留当前 installation-scoped Config。新增 identity round-trip 回归；尚未形成真实桌面跨进程证据，因此不关闭完整 Config 或 Windows E3。

2026-08-22 Minori Auto 增量：活动消息等待支持同 token `Input`/`Time` modality 重绑定，CLI、Manager 和 RuntimeWorld mirror 保持单一权威 token，其他重复注册继续 blocking。最快 Auto 以一个 10 ms timing unit 运行。真实八包首路线在正文阶段无周期性 Enter，只在 choice active 后确认一次；运行完成 25552 fixed steps、25557 个 GPU frame、50 条物理输入、terminal、snapshot round-trip 和零 diagnostic，coverage hash 与既有完整路线一致。六个关键 checkpoint 的人工视觉检查通过。该证据只关闭持久 Auto 的首路线 Headless E2；Skip 整路线、正式音频听审、Config 剩余行为、鉴赏和 Windows E3 仍开放。

同日 Control 增量：有效 Control gate 会把活动消息同 token 重绑定为 10 ms `Time`，释放后可恢复为 `Input`；media/presentation/provider fence 保持原完成边界。真实八包首路线在正文阶段无周期性 Enter，完成 25496 fixed steps、25498 帧、28 条输入、terminal、snapshot round-trip、自然解锁和零 diagnostic，coverage hash 与既有完整路线一致。音频 master peak 为 0.989372，output overload/underflow 为 0；标题、剧情和返回标题三个 checkpoint 的人工视觉检查通过。该证据关闭 Control 首路线 E2，不替代正式音频听审或 Windows E3。

同日 Skip 增量：首次真实运行严格阻断并暴露 Config slider x 区间吞掉 Auto/Skip 与部分 Font hit region；修复后非 slider y 坐标继续进入原程序 hit map，重叠区回归通过。真实八包从 Config UI 选择 Skip、Apply、释放 Control，再用游戏菜单启用持久 Skip；运行完成 24901 fixed steps、24906 帧、52 条输入、terminal、snapshot round-trip、自然解锁和零 diagnostic，coverage hash 与既有完整路线一致。音频 master peak 为 0.989507，output overload/underflow 为 0；六个 checkpoint 的人工视觉检查通过。持久 Skip 首路线 E2 已关闭，Config 其余行为、正式音频听审、鉴赏和 Windows E3 仍开放。

同日 backlog 增量：三记录 VM cursor 边界和 provider idle retention 回归通过。首次真实视觉检查发现 idle tick 清除 retained text，修复后 `clear_text` 只随实际 system page/cursor/input/restore 变化或 terminal 提交。真实八包短程完成 771 fixed steps、776 帧、55 条输入、snapshot round-trip 和零 diagnostic；打开、两次上翻与关闭的四个 checkpoint 显示 gauge 位置和三条不同历史文字同步变化，关闭后恢复当前消息。该运行未到 terminal，只关闭 backlog 多记录翻页的定向 E2。

同日 Config 文字阴影增量：应用后的选项直接控制公共 typed text presentation 的既有 2 px 黑色 outline，关闭时提交 `None`，不改变 shaping、字体、layout 或 Renderer2D 主路径。真实八包短程完成 384 fixed steps、388 帧、34 条物理输入、snapshot round-trip 和零 diagnostic；Config 开关前后及首条剧情消息的视觉检查通过。该运行未到 terminal，只关闭文字阴影定向 E2。

立绘动态 coverage 另行记录：IDA 已确认 `.char trans` 的阻塞式线性透明度合同和 `.char vis` 布尔规则。v22 runtime/provider 测试覆盖等待、线性中间帧、完成和 snapshot；合成 256→128→0 序列又通过 Headless WGPU capture 与人工视觉检查。真实脚本 census 未出现 `trans/vis`，所以这项只计合成动态立绘 E2，不计入真实路线。同一 clean Release build、package、profile、物理输入、adapter 与 driver identity 连续三次完成十分钟 120 Hz GPU 测量；每次均有 72000 个正式 sample，deadline miss、audio underflow、full resync、trace dropped、稳定段 upload/readback/allocation 与 memory growth 为 0，runtime p99 为 0.2725–0.2938 ms，presentation p99 为 0.81456–1.11064 ms。该静态标题负载关闭正式 GPU 性能 E2，但不覆盖完整路线媒体、音频听审或 Windows E3。

同日最新同身份单次运行完成 33490 fixed steps、34108 帧、16951 条输入和 33 个 checkpoint，首个 choice、实际 post-choice 分支与不可由 Control 跳过的结局媒体已进入同一份通过报告。global progress rollback snapshot 使用独立 versioned section，restore 后首个 message 与 retained scene 在同一 typed transaction 提交；terminal、snapshot round-trip、user save/restore 和自然 unlock 均成立，diagnostic 为空。模型已查看全部 33 个 required frame，`prepare-review` 自动门禁通过；完整 WAV 仍待逐段听审，`validate-review` 正确返回 `ASTRA_HEADLESS_REVIEW_BLOCKED`。此前两次重复运行继续提供确定性证据，本次运行不替代该边界。

同日 v21 真实八包完整路线进一步覆盖 message voice 与 backlog 当前记录重播：33553 fixed steps、34172 帧、16957 条物理输入、34 个 checkpoint、terminal、snapshot round-trip、用户 save/restore 和零 diagnostic 全部通过。重播 checkpoint 前后画面稳定，完整 WAV 为 27311104 frame，master peak 0.989529，output overload/underflow 为 0。该证据提升当前单路线的 Headless E2 覆盖，不解除具名人工音频 review、第四条 clear route 后鉴赏或 Windows E3 blocker。

Headless review contract 已硬切到 v3：review 同时绑定 run report、review bundle 和 selected audio role/path/hash，CLI validator 会复算实际 artifact hash。缺失、额外或失败的完整音频 verdict、bundle 漂移和 artifact 漂移都会在 protocol、CLI validator、Python platform acceptance 与 release preflight 阻断。v193 私有听审 catalog 以 10 个连续区间覆盖全部 27260416 frame，源 hash 匹配 bundle，但尚未完成具名逐段听审，因此本项仍不提升正式 review 状态。

同日后续更新：checkpoint capture 已强制提交当前 Scene2D，decoded video 改走公共 `SceneCommand::VideoFrame`。真实 movie checkpoint 已检查 decoded frame、比例、文字上层和残留；其间修正了 transient draw blend 与公共 WGPU atlas 连续帧 placement 生命周期。Kira main track 的显式 peak limiter 让完整路线 master output peak 保持在约 0.990，output overload 与 underflow 均为 0，同时保留约 2.71 的 pre-master mix 诊断；完整 WAV 非静音且低于 i16 full scale。标题启动路线已覆盖用户 save/restore 与 terminal；global progress、自然鉴赏和 Windows E3 仍开放。

2026-08-08 的开发复用 package 已在同一 build/profile/device 上完成 36,000 tick Windowed E2、Perfetto 和独立 29-checkpoint 视觉线路：fixed tick p99 3.082 ms，deadline debt、audio underflow 与 scene/PCM copy counter 为零，实际帧可见启动 logo、标题和黑底序章 UI/正文。该 run 未到 terminal，且不是冻结 clean Release identity，只作为 E3 候选与回归证据；正式性能 evidence 等级不提升。

同日的 RFVP/Astra 颜色差异审计确认 RFVP 上游原版渲染是视觉权威；此前尝试统一 premultiplied source-over 会改变产品色值，已撤销。hosted fork 固定到 `a1d9abd201e6a0baf6259309543da5166a814c1c`，保留 typed ownership，并恢复上游原版 renderer 行为。Family ABI v8、Provider ABI v4、Headless CPU、Manager GPU 与 PlatformHost GPU 原样传递 nearest/linear；FVP adapter 只映射上游 draw state，不改写 Astra 通用 renderer。600 tick 同物理输入 Headless GPU 运行通过，599 个可对照帧均已生成；第 60–599 帧相对上游 software oracle 的平均 RGBA MAE 为 2.211，最大 2.389，标题花瓣、光点、右下花树和菜单辉光均可见。相同 build/input 重跑的 scene、raster 与 audio stream hash 完全一致，音频为 479744 frame、无 clipping，原先由 wall-clock 补水导致的 artifact duration overflow 已由 Headless 固定 tick Kira 驱动消除。该 oracle 属于旧 ABI 历史 evidence；v8 必须重跑。它不是 Windows native GPU capture，clean Release Perfetto 与人工 E3 仍未完成，因此 coverage 等级不提升。

2026 年 8 月 9 日继续按 RFVP Headless oracle 校准 hosted adapter。fork revision
`ce921717f043a8a035eadff1f41fc060c7de7c3c` 在 hosted 构建内使用与上游相同的四个固定字体，
删除 Astra 侧 Noto 字体覆盖，并补回上游在 dissolve 完成边沿、正常帧 tick 之前执行的同步
零时长 VM tick。首条真实线路两侧均完成 33,682 帧；按 oracle 零基帧 `N` 对 Astra 一基
fixed step `N+1` 比较，dHash p50/p95/p99/max 为 1/4/5/26，平均通道差异为
0/1/1/1，512 帧 RGBA SHA 完全一致。完整线路已无语义帧边界偏移，但软件栅格器仍有小幅
像素差异，不能写成逐像素完全一致。最终 hosted revision 的 dirty Headless Perfetto trace
实测 scene/PCM copied bytes 均为零，audio underflow 为零；Headless 未打开物理音频端点。
clean Release Windowed Perfetto 和人工 E3 继续 blocking，因此 coverage 等级不提升。

同日人工 E3 启动检查修复 Manager auto-probe 的 `fvp.pack_paths` 漏项和完整 Windows
PlatformHost 抢先创建 Winit event loop 的冲突。pack 列表现在只取 HCB 同目录的已绑定 VFS
条目，并仅注入本次 provider open；Windows Kira worker 改用无窗口 media-service host 承载
audio/decode。开发复用 Release 包已打开真实 Manager 窗口并保持响应，人工路线确认仍未完成。

| 模块 | Design | Contract | Public API | Data Format | Test Scenario | Release Gate | Manual |
| --- | --- | --- | --- | --- | --- | --- | --- |
| EngineCore | [module](../modules/engine-core.md) | [runtime](../contracts/runtime.md) | `Engine/Source/Runtime/astra-runtime` implemented, `Engine/Source/Runtime/astra-engine` Rust dylib facade implemented；Runtime v3 包含 inverse journal/overlay、增量 root、compiled dispatch 和 conflict-DAG executor | shared `astra-package` save container；RuntimeSnapshot 包含 StableId generator、typed component、完整 Event/Await/Delayed queue、mutation/effect trace；`RuntimeReplayTranscript` 包含 input/await/provider output/checkpoint | `cargo test -p astra-runtime` 覆盖 tick/action/access/1-8 worker/save/replay/event ordinal，`cargo test --workspace` 与 clean Release 72,000 帧产品性能运行通过 | runtime determinism, run-to-quiescence flat StateMachine transaction, Await replay policy, save/load continuation, provider-free replay, bounded candidate work, structured logs, Rust dylib facade | [operator](../manual/operator-guide.md) |
| Observability | [release observability](release-observability.md) | [logging/crash](../contracts/logging-observability.md) | `astra-observability`、`astra-crash-reporter`、CLI/Player/bundled Player host integration implemented；非 Windows native crash 不在本轮范围 | `astra.log_event.v1`、`astra.crash_bundle.v1`、standalone manifest v2、`AstraPlayer.config.json` v2、[crate classification](logging-coverage.json) | `T-OBS-CORE-01`、`T-OBS-COVERAGE-01`、`T-OBS-CRASH-WIN-01` | stable event schema、file/ring bounds、critical mirror、privacy redaction、Windows reporter role/hash/self-test/handshake/tamper blocking | [operator](../manual/operator-guide.md), [plugin guide](../manual/plugin-developer-guide.md) |
| Target Model | [architecture](../product/architecture.md) | [data](../contracts/data-formats.md) | `Engine/Source/Platform/astra-target` implemented, [target/platform](../implementation/target-platform.md) | `project.yaml targets`, `astra.target_manifest.v1` package section | `cargo test -p astra-target`, `cargo test -p astra-cli --test target_platform` | target manifest | [operator](../manual/operator-guide.md) |
| Plugin ABI | [architecture](../product/architecture.md) | [plugin](../contracts/plugin-abi.md) | `Engine/Source/Runtime/astra-plugin-abi` and `Engine/Source/Runtime/astra-plugin` implemented, binding v2 shared validator, [provider API](../implementation/provider-plugin-api.md) | plugin YAML, `astra.plugin_extension_registry.v2`, `astra.provider_policy.v2`, dependency graph | plugin load/unload, registration rollback, binding hash/context drift, FFI action, extension registry and logging scenario | fingerprint/capability/target/profile, exact policy-registry binding, `plugin.extension_registry`, `plugin.dependency_graph`, plugin structured logs | [plugin guide](../manual/plugin-developer-guide.md) |
| Gameplay Runtime Provider | [architecture](../product/architecture.md), [runtime blueprint](../implementation/game-runtime-provider.md) | [game runtime](../contracts/game-runtime-provider.md) | `astra-plugin-abi` 提供 ABI v4 typed lifecycle 与分类 live output；通用 bytes action、persisted output envelope 和 recorded provider output 已删除。每 Session 独占 RuntimeWorld/ordered mailbox；NativeVN live state 是有界 host projection，save 仅保留 `runtime.world` v4 | runtime provider descriptor, `astra.runtime.save_blob.v4`, typed create/prepare/probe/open/step/save/restore/shutdown DTO, release check descriptor, `RuntimeEditorMetadata`, `FfiRuntimeProviderRegistration` | `T-S3-RUNTIME-PROVIDER-01`、`T-S3-RUNTIME-BATCH-01`、`T-S4-EDITOR-RUNTIME-PROVIDER-01`、`T-S5-GAME-RUNTIME-01`；本轮全量门禁尚未重跑 | explicit binding, typed sequence/bounds, full snapshot restore continuation, provider-free typed replay, per-Session poison isolation, global worker budget | [operator](../manual/operator-guide.md), [plugin guide](../manual/plugin-developer-guide.md) |
| Asset Pipeline | [module](../modules/asset-pipeline.md), [VFS blueprint](../implementation/asset-vfs.md) | [data](../contracts/data-formats.md), [asset VFS](../contracts/asset-vfs.md) | `astra-asset`、`astra-cook`、`astra-package`、`astra-release` 的 typed dependency、bounded incremental cook、VFS/container authority 和 product package data flow 已完成本轮生产加固 | asset sidecar YAML with dependencies, `astra.cook_manifest.v2`, host-local content cache, binary package, `astra.schema_registry.v2`, `astra.scenario_refs.v2`, `VfsUri`, `asset.vfs_manifest`, `asset.catalog`, prefix/layer/entry/whiteout DTO, local root capability | `cargo test -p astra-asset --all-targets`, `cargo test -p astra-cook --all-targets`, product cook test, `T-S2-ASSET-02`, `T-S2-PACKAGE-AUTHORITY-01`, `T-S2-VFS-01` | graph/capacity/cache/cancel/atomic commit, package integrity, required-section schema authority, scenario identity, release input, VFS context/conflict checks, path/payload redaction | [creator](../manual/creator-manual.md) |
| Media Runtime | [module](../modules/media-runtime.md) | [media](../contracts/media.md), [performance](../contracts/performance.md) | `astra-media-core` 提供 Renderer2D/FilterGraph contract 和 transactional CPU executor；`astra-media` 持有 decode、字体和 owned PCM asset。`astra-audio-kira` 是默认 mixer provider，Kira 不启用 CPAL/Symphonia feature；PlatformHost 只持有 endpoint。旧 `ProductionAudioMixer` 和音频 HostCommand refill 已删除。Headless 与各平台 host 共用 `AudioServiceSession`、typed command 和 `AudioOutputLane`；WebAudio 由 AudioWorklet 请求 chunk | `astra.audio_timeline.v1`、audio chunk telemetry、FilterGraph report、decode result、性能报告、PNG/WAV artifact manifest | `cargo test -p astra-audio-kira --tests`、`cargo test -p astra-platform-common --tests`、`cargo test -p astra-emu-family-support --lib`、`cargo test -p astra-player-vn --test product_audio_host`、Web code-check、Windows host tests、正式 Perfetto/Windowed E2 | owned PCM、无锁 chunk queue、同格式 F32 allocation move、Kira/Family 单测、Web code-check 与 Windows workspace all-target 已覆盖；Linux/Android 交叉工具链当前缺失，device loss 精确续播、真实游戏 10 分钟 Perfetto 和 Windowed E2 仍 blocking | [operator](../manual/operator-guide.md) |
| Runtime UI | [UI backend](../implementation/ui-backend.md), [Migration 12](../migrations/astra-ui-backend-split-migration.md) | [UI](../contracts/ui.md), [UI component plugin](../contracts/ui-component-plugin.md) | `IN_PROGRESS`：`astra-ui-core`、`astra-ui-yakui`、`astra-ui-plugin-abi`、Scene2D Mesh、UIA/ARIA 与性能 gate 已落地 | `astra.ui_*` schemas、`UiRenderFrame`、`UiSemanticSnapshot`、`astra.target_manifest.v2`、UI/component package sections | `T-S2-UI-BACKEND-01`、`T-S3-UI-SCRIPT-01`、`T-S3-UI-EXT-01` | explicit binding、input consumption、resource generation、signed component 与 Headless E2 已落地；Windows/Web E3 待闭合 | [creator](../manual/creator-manual.md), [plugin guide](../manual/plugin-developer-guide.md) |
| AstraVN | [module](../modules/astra-vn.md), [script spec](../modules/astra-vn-script.md), [grammar IR](../implementation/astra-grammar-ir.md), [presentation](../modules/astra-vn-presentation-model.md), [commands](../modules/astra-vn-standard-commands.md), [system UI](../modules/astra-vn-system-ui-profile.md), [UI backend](../implementation/ui-backend.md) | [script-vn](../contracts/script-vn.md), [UI](../contracts/ui.md), [player automation](../implementation/astra-vn-live-player-automation.md) | Migration 6 frontend、Migration 9 shared policy 与 Migration 12 Yakui/UI Blueprint/Controller/component ABI 已落地；Classic/Modern 产品页统一走 `CompiledVnProject`、AstraText 与 Scene2D | `CompiledVnProject`、UI blueprint/binding/controller/theme/component sections、`UiSemanticSnapshot`、`UiRenderFrame` | `T-S3-SCRIPT-01`、`T-S3-SCRIPT-02`、`T-S3-PRESENT-01`、`T-S3-SYSTEM-01`、`T-S3-UI-SCRIPT-01`、`T-S3-UI-EXT-01`、`T-S3-PLAYER-AUTOMATION-01` | Headless E2 与 authority tests 已落地；Classic/Modern Windows/Web E3、performance 与 accessibility formal evidence 待闭合 | [creator](../manual/creator-manual.md) |
| NativeVN Flagship Project | [migration](../migrations/nativevn-flagship-demo-migration.md), [project](../../Examples/NativeVN/README.md) | [script-vn](../contracts/script-vn.md), [content schemas](../../Examples/NativeVN/Schemas/) | 旗舰内容直接替换 `Examples/NativeVN`；`.astra` canonical story、project descriptor、localization、UI/theme/controller、180 条用户授权中文配音、283 个 asset sidecar 与 package section 接入真实 Cook | `CompiledVnProject`、`astra.cook_manifest.v2`、cooked sections、内容 provenance/review/voice release schemas | `T-S3-FLAGSHIP-DEMO-01`：内容校验、工具单测与真实 `astra-cli cook`；本轮不执行 Player/Runtime 测试 | Cook 成功只形成 cross-module Cook evidence，不代表 `.astrapkg` 或运行验收；Windows/Web E3 仍 blocking，`S3-FLAGSHIP-DEMO-01` 保持 `IN_PROGRESS` | [project status](../../Examples/NativeVN/STATUS.md) |
| NativeVN Engine Test Profile | [Stage 3 status](stages/stage-3-astra-vn.md) | `astra.vn.presentation_provider_manifest.v2`、`astra.vn.profile_manifest.v1` | `minimal` profile 固定为低预算 Engine 测试入口，fixture 包含 choice、全部 system page、UI/controller/theme、字体与完整本地化，不携带重型媒体 | cook manifest、package section、submitted/rasterized frame evidence、checkpoint result | `T-S3-GAME-TARGET-01`：真实 cook/package/Headless 回放与 release isolation | 仅形成 Engine E2；release gate 必须阻断 `minimal`，不得替代 `classic`/`modern`/`advanced-vn` 或 Windows/Web E3 | [test matrix](stages/stage-test-matrix.md) |

补充：AstraVN/TsuiNoSora coverage 中的 `tsuinosora.cast_source_map_report.v1` 当前覆盖两条公开 synthetic 路径：手写 `tsuinosora.cast_map.v1` 的 source/container entry/hash 映射，以及从 Director `KEY*`/`CAS*` cast map 通过 child resource id、FourCC、container entry id 和 extracted payload hash 派生的 source map；手写或外部 reader sidecar 声明 `source_hash` 时，必须匹配实际 extracted source asset，否则 blocking。`tsuinosora.route_graph.v1` 当前覆盖脱敏 route/terminal/choice sidecar，带正文类字段或不安全 symbol 时必须 blocking。`tsuinosora.script_source_map.v1` 当前覆盖外部 reader 产出的脱敏 route/source/line/hash sidecar，以及 `extract-readable` 对可读或短 binary-header wrapped mapped `Lscr` 自动生成的 `director_lingo_source_map.json`；带正文、bytecode、本地路径、无效 hash、不安全 symbol、不安全 reader id/hash/output contract、声明 source hash 与现有 report-relative source 文件不一致、route line 超出声明 source line_count、未声明 route source 或 route/source hash 不一致时必须 blocking。同一 route 同时来自抽出的 `.ls` 文本和 reader sidecar 时，`tsuinosora.script_source_map_report.v1` 优先保留 reader source-map evidence 和脱敏 reader id/hash evidence，避免重复 coverage。`tsuinosora.director_lingo_map.v1` 覆盖 `Lctx`/`Lnam`/`Lscr` 的 hash-only preflight；`Lnam` 只输出 entry count 和 table hash，不输出 symbol/name 字符串；`Lscr` 不是可直接抽取文本且没有合规 source-map sidecar 用匹配的 `director_lingo_map.json` source/hash 和 Lscr `script_resource_id`/`script_payload_sha256` 证明 route coverage 时，`tsuinosora.script_source_map_report.v1` 必须因 unsupported Lingo bytecode blocking；缺失 resource 字段、未知 resource id 或 hash mismatch 也会 blocking，匹配时作为公开 synthetic 中间层证据通过。`XFIR` 只接受 verified exact wrapper 中的 RIFF/RIFX payload，wrapper size 必须覆盖整个文件，并只写 decoded hash/size；opaque、压缩、尾随未验证 bytes 或不可读 Shockwave container 必须 blocking，不能退回 RIFF/RIFX 线性扫描。缺 child resource 抽取结果或 hash 冲突也必须 blocking。

补充：`tsuinosora.conversion_manifest` release gate coverage 现在要求 `resources` 至少包含一条 converted resource evidence；route 全部 covered 但 `resources` 为空会 blocking，避免用路线 coverage 冒充真实素材转换证据。每条 resource 必须带 source/native 相对路径、classification、source hash、converted hash 和正 byte size。

补充：`tsuinosora.director_lingo_map.v1` 也记录 `Lctx` entry count/table hash；`Lctx` 和 `Lnam` 都只作为脱敏结构证据，不输出 table payload。Malformed `Lctx` table 或未终止 `Lnam` table 会 blocking，避免把坏 Lingo table 当作 route coverage 前置证据。

补充：`tsuinosora.director_cast_map.v1` 现在会阻断同一 `CASt` 被多个 `CAS*` library/slot 绑定的冲突，避免 `tsuinosora.cast_source_map_report.v1` 用不唯一 member 生成素材/route evidence。

补充：`tsuinosora.cast_source_map_report.v1` 会阻断手写 cast map 或外部 director cast map sidecar 中的 payload、正文或 bytecode 字段；diagnostic 只保留字段路径，不保留字段值。

补充：TsuiNoSora package section release gate 现在会统一阻断 `tsuinosora.*` section 中的 payload-like 字段泄露；正文、脚本文本、source text、content、payload body、bytecode 和 source payload 不能进入发布包报告，`redaction.payload: omitted` 除外。

补充：`tsuinosora.asset_analysis` release gate 现在还要求至少一条 analyzed asset evidence；空 `assets` 即使 `status: pass` 也会 blocking。

补充：`tsuinosora.director_cast_map.v1` 支持显式 `tsuinosora.director_cast_member_metadata.v1` 的脱敏读取，覆盖 kind、route id、command id、anchor、bounds 和 metadata hash；这些字段会继续进入 `tsuinosora.cast_source_map_report.v1`，普通 `CASt` payload 不会被输出。

补充：`tsuinosora.reference_evidence` 对应的 `tsuinosora.visual_reference_report.v1` 现在会校验默认 `Title.png`/`Game.png` 的固定尺寸和 hash；缺文件、PNG 不可读、hash mismatch 或 dimensions mismatch 会 blocking，report 不新增商业截图输出。

补充：TsuiNoSora visual acceptance 现在还覆盖 `tsuinosora.visual_screenshot_capture_report.v1` 和 `tsuinosora.visual_comparison_report.v1`。截图文件、差异图和调试视频只能留在 ignored `.local`；可提交 report 只记录 checkpoint/route/region id、尺寸、hash、差异指标、自动采集执行摘要、same-run capture roles、视觉 review hash 和 diagnostic。

补充：Director cast metadata 的 anchor/bounds 必须是可验证数值；anchor 非数值或 bounds 负尺寸会 blocking，避免布局证据缺失后继续生成 source-map evidence。

补充：Director cast metadata 中 `kind: character_atlas` 必须携带 parts；part id、pose、expression、anchor、crop、layer、mouth/eye state compatibility 和 fallback 会在 Director cast report 与 cast source-map report 中保留为脱敏证据。

补充：`tsuinosora.route_graph_report.v1` 和 `tsuinosora.script_source_map_report.v1` 现在会阻断同一 `route_id` 的 terminal/choice signature 冲突，避免不唯一 route coverage 进入 NativeVN package input。

补充：route graph 和 script source-map report 也会阻断同一 route 内的重复 choice id，避免重复选择证据写入 `.astra` option key 或 scenario `player_input choose`。

补充：Stage 3 gate 的 script source-map fallback 只在 route graph 缺失时使用；坏 route graph sidecar 的 payload、unsafe symbol、coverage 或 duplicate diagnostic 会继续阻断 conversion，不能被 fallback 覆盖。

补充：`local-gate` 当前不允许显式 routes 绕过 route evidence；routes 必须由 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 派生，显式 routes 会 blocking 且不会写 NativeVN package input。

补充：`tsuinosora.script_source_map_report.v1` 对 unsupported Lingo bytecode 使用 resource 粒度覆盖；同一个 Lingo map 中只覆盖一个 `Lscr` 而遗漏其它 `requires_bytecode_reader` resource 时必须 blocking。

补充：`tsuinosora.director_resource_map.v1` 会把 `mmap` free entry 计入 `free_resource_count`，但不会把它们当作有效 resource、tag coverage 或 payload evidence；这用于避免旧 Director free-list entry 让可读资源图误阻断。
`tsuinosora.native_asset_rearrange_report.v1` 当前作为本地 gate evidence，证明 Asset analysis pass 后写入 `local_work_root/native-assets/` 的 source/native 相对路径、classification、source hash、converted hash 和 byte size；该 report blocked 时 conversion report 必须 blocked。

NativeVN package input 当前保留 route graph/source map 中的 sanitized choice id，生成同名 `.astra` option key 和 scenario `player_input choose.value`；多 choice route 按顺序生成连续 choice state，避免用 synthetic 单选覆盖真实 route evidence。

补充：NativeVN package input 还会对显式 routes 执行写入前校验；不安全 symbol、非 covered coverage、重复 choice 或冲突 route signature 会阻断，避免无效输入生成 story 或 scenario refs。

TsuiNoSora `stage3-gate` 当前会把 route-bound cast source map member 通过 source/native hash 映射到 `native-assets/`，生成 patch/windows `mount_assets` 并写入 conversion report；`local-gate` 从 report 派生 routes 时会保留 choices 和 mount evidence。

| AstraRPG | [module](../modules/astra-rpg.md), [runtime blueprint](../implementation/astra-rpg-runtime.md) | [rpg](../contracts/rpg-trpg.md), [game runtime](../contracts/game-runtime-provider.md) | `Engine/Source/Modules/AstraRPG/` planned；`AstraRpgRuntimeProvider` planned；`rpg.trpg` planned profile, not top-level module | `rpg.*`, `rpg.trpg.*`, `rpg.net.*` planned package/save/report sections | `T-S7-POLICY-01`, `T-S7-RPG-PROVIDER-01`, `T-S7-RPG-AI-TOWN-01`, `T-S7-RPG-TRPG-01`, `T-S7-RPG-CP2020-01`, `T-S8-RPG-NET-REPLAY-01` | `runtime_provider.astra_rpg`, `rpg.policy_bundle`, `rpg.intent_validator`, `rpg.agent_provider_free_replay`, `rpg.trpg.dice_determinism`, `rpg.trpg.seat_authority`, `rpg.trpg.transcript_redaction`, `rpg.cp2020.local_private_adapter`, `rpg.net.provider_free_replay` planned | [creator](../manual/creator-manual.md), [operator](../manual/operator-guide.md) |
| AstraEditor | [module](../modules/editor.md) | [AI/MCP](../contracts/ai-mcp.md), [game runtime](../contracts/game-runtime-provider.md) | [Qt/Rust bridge](../implementation/editor-workflow.md) planned；runtime-provider-aware shell is `REOPENED_SPEC` | layout preset YAML, plugin enablement state, `RuntimeEditorMetadata` | creator workflow, plugin manager scenario, `T-S4-EDITOR-RUNTIME-PROVIDER-01` | editor package gate, plugin manager gate, runtime provider metadata redaction, PIE provider/profile handoff | [creator](../manual/creator-manual.md) |
| AI/MCP | [module](../modules/ai-mcp.md), [Asset VFS](../implementation/asset-vfs.md) | [AI/MCP](../contracts/ai-mcp.md), [asset VFS](../contracts/asset-vfs.md) | [AI/MCP API](../implementation/ai-mcp-runtime.md), [provider profiles](../implementation/ai-provider-profiles.md), [runtime memory](../implementation/runtime-ai-director-memory.md), [MCP context](../implementation/mcp-context-tooling.md) planned；VFS alignment is `REOPENED_SPEC` | audit log, encrypted debug trace, AI draft sidecar, runtime memory ledger, Context Pack, `ai.model_bundle_manifest`, `ai.generated_artifact.*` save sections, ModelBundle VFS locator | trusted session, runtime director, memory compaction, MCP context scenario, ONNX ModelBundle package/VFS lookup, `T-S4-AI-VFS-01` | provider profile, ONNX ModelBundle, runtime VFS mount, fixed EP evidence, generated artifact save, runtime memory policy, context permission, provider-free replay | [plugin guide](../manual/plugin-developer-guide.md), [creator](../manual/creator-manual.md) |
| Platforms | [platforms](../platforms/README.md) | [release](../contracts/release-gate.md) | Migration 8 `IN_PROGRESS`；`astra.platform_host_profile.v3` 已拆分 `audio_mixer=kira` 与平台 `audio_output`，旧 profile 直接拒绝。Windows、Linux、macOS、Android、Web 与 Headless 源码均接入 typed `AudioOutputLane`，不再经 HostCommand refill；各平台实机验收尚未闭合 | `astra.platform_host_profile.v3`、`astra.headless_host_profile.v3`、`astra.user_input_sequence.v1`、`astra.platform_profiles.v3`、`astra.platform_capability_report.v2`、`astra.platform_host_conformance_report.v1` | platform contract、workspace all-target check、Web code-check、Windows lane tests；Linux/macOS/Android cross-target 与真实 native tests 待执行 | profile v3、typed lifecycle、全平台 lane 源码收束和 Windows/Web 可执行检查已覆盖；Linux/Android 本机交叉工具链缺失，设备恢复与 E3 仍 blocking | [operator](../manual/operator-guide.md) |
| AstraEMU | [module](../modules/astra-emu.md), [family research](../emu/README.md), [runtime framework](../implementation/astraemu-legacy-runtime-framework.md), [EmulatorCore mapping](../implementation/emulator-core-state-machine.md) | [game runtime](../contracts/game-runtime-provider.md), [legacy runtime provider](../contracts/astraemu-ipc.md), [asset VFS](../contracts/asset-vfs.md), [media](../contracts/media.md), [script](../contracts/script-vn.md) | `IN_PROGRESS`：FVP hosted v5 以 pinned thin fork 的单 delta、session-owned state 和 host-bound named-audio resource port 运行；公开 dynamic Headless E2 已分别覆盖输入可见提交和 URI-only audio 的 WAV/meter 输出。2026-08-02 的授权本机安装 signed dynamic Headless E2 已通过 300 step、170 frame、snapshot round-trip、受限 VFS 账本与一个后期 checkpoint 视觉检查；另一次复用既有 12 条物理输入的 1,261 step 运行完成且无 diagnostic、产生非静音非削波 WAV，但两个转场 checkpoint 相同。随后修复 fork hosted-core 的键盘 event 转发后，4,200 step 真实确认序列显示完整标题菜单并输出非静音 WAV；5,100 step 的首个菜单项 click 已进入黑场媒体/转场阶段，输出持续非静音 WAV 且无 diagnostic，但尚未形成正文视觉或 terminal。2026-08-03 原生 10 分钟 soak 的 RFVP core p99 为 2.923 ms，adapter 长帧却达到秒级，并累计 656 次 audio underflow；对照 RFVP `0.5.0` 后确认动态纹理从原位更新退化为跨 ABI 像素传递、新 generation 和整纹理重传。通用 WGPU atlas 只有容量不足或碎片化时才 repack，当前证据尚未证明每次变化都发生全 atlas 重建。该 soak 失败，路线、完整视频/PTS、稳定 subresource update、独立 audio producer 和平台 host 均保持开放。其余既有 family ABI、RuntimeWorld、Slint/WGPU、CLI native/headless、Library v7、discovery 与 metadata work 保持不变。离线 HTTP fixture、中央兼容性数据仓、完整 UI 自动化、商业 VNDB license gate、正式平台签名、完整 media parity、Windows/Android E3 仍未完成 | `astra.emu.*` schema、Library v7 migration（play_session/compatibility_entry_cache/compatibility_sync_state）、`astra.emu.compatibility.v2` JSON Schema（VNDB 唯一权威源，(vID,rID) 版本精确） 导出、metadata snapshot/match evidence、Bangumi sync diagnostic、FVP coverage/parity、VFS audit、provider/UI/platform/translation/Luau evidence | metadata/core crate check、FVP provider unit tests 和局部 signed dynamic Headless E2 已通过；完整 workspace、真实游戏路线、真实 UI 和平台证据仍待正式门禁，不从局部结果外推 Stage 完成 | `emu.release_manifest`、`emu.provider_binding`、`emu.ui_host_identity`、`emu.metadata_license`、`emu.metadata_privacy`、`emu.fvp_coverage`、`emu.fvp_parity`、`emu.trusted_luau`、`emu.translation`、`emu.platform.*` 保持 fail-closed | [operator](../manual/operator-guide.md)、[Manager metadata](../manual/astraemu-manager-metadata.md) |
| Headless Test Backend | [Migration 11](../migrations/headless-platform-test-backend-migration.md)、[platform host](../implementation/platform-host.md) | [media](../contracts/media.md)、[performance](../contracts/performance.md) | `E2_DONE_E3_IN_PROGRESS`：v3 GPU policy、60 Hz Runtime/120 Hz presentation cadence、稀疏提交/渲染、双流证据、lazy package、bounded cache、retained atlas、异步 timestamp/readback ring 和 Perfetto Trace Event writer 已实现；TsuiNoSora 的 800×600、1920×1080 和完整 Y 路线均已完成集显 E2 | `astra.headless_host_profile.v3`、`astra.user_input_sequence.v1`、checkpoint/artifact/run/review/preflight v2、`astra.performance_budget.v1`、`astra.performance_report.v1`、`astra.performance_trace_manifest.v1` | CPU all/checkpoints correctness、GPU policy、cadence、package range/source mutation、atlas/readback/timestamp/profiler overhead；800×600、1920×1080 与完整 Y 路线的 final E2 已冻结 | Headless 仍只形成 E2；GPU 缺硬件、软件 adapter、timestamp query 缺失、trace 或 identity 漂移、预算 blocked 都必须失败；Windows E3 仍未完成 | [operator](../manual/operator-guide.md) |

补充：Minori GameView 已把舞台内真实右键接入 system-menu contract。v11 不再把右键等同于 Save：family 先发布原版层级，Host 回送选择或取消，随后才执行 Save、Load、Config、消息框或 Auto/Skip。Manager 在菜单交互期间保留底层 gameplay wait；定向回归通过。新的 v11 Release Sandbox 复测、完整路线、媒体/音频审查和 Windows E3 仍开放。

2026-08-30 后续补充：开发签名 Release v24 通过同一 v11 hierarchy 和 82 条物理输入打开 Save、Load 与 gameplay Config，再分别返回剧情。报告为 138 fixed steps、9 个呈现帧、8 个 checkpoint、零 diagnostic；全部页面已视觉复核，页面前后的四张 gameplay PNG 字节一致。Load 与 Config 的页面所有权回归已进入 Minori tests。该证据是定向 Headless E2，不替代实际存档跨进程、退出确认、Release Sandbox、性能门禁或 Windows E3。

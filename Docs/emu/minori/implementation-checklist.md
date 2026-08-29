# Minori Implementation Checklist

## Family API v11 right-click system menu (2026-08-30)

- [x] `LegacySystemMenuRequestV1` and bounded `LegacySystemMenuTransactionV1` are part of the hard-cut Family API v11 contract and have explicit `StableAbi` wire representations.
- [x] Manager promotes a pressed physical `pointer.secondary` edge to `Open`, carries the latest bounded pointer coordinates, and removes the duplicate pressed edge before invoking Minori.
- [x] Minori publishes the observed title/gameplay hierarchy. Windows native uses the platform context-menu provider, Manager renders the same transaction as a Slint overlay, and Headless navigates it only through physical input records.
- [x] Save, Load, Config, message-panel visibility and Auto/Skip are selected through the returned menu item. Ambiguous input, active choice/media, completion sharing, duplicate menus, stale results and missing services fail closed.
- [ ] Windows E3 remains open: the Sandbox direct native run reached real scenes and completed its windowed route with audio explicitly disabled, but the WASAPI default output was unavailable and the read-only share prevented artifact recovery; no E3 artifact is claimed.

## 增量游标与 FFmpeg 复核（2026 年 8 月 27 日）

旧 private-profile/cache identity 下的八包 full verify 仅保留为迁移历史。当前 key-file/streaming identity 已重新完成无明文缓存的八包 full verify，并通过合成 source 计数回归确认重复 range read 会重新访问密文 source；峰值内存的独立规模门禁仍未关闭。

- [x] `IncrementalMediaPlayback` 校验完整播放配置、单调 tick、轨道声明、资源标识、PTS/duration、尺寸、PCM 格式和 bounded queue；显式执行视频 lead/lag 与 `Block`/`Drop` 策略，并记录迟到帧计数。
- [x] 共享 AstraMedia FFmpeg provider 完成真实影片的 demux、逐 packet decode、PCM resample、seek/cancel、Control 跳过和 media fence；Minori 没有保留手写 AVI/WMV decoder 或平台 fallback。
- [x] 当前签名 release plugin 的 title→config→movie→skip→title slice 报告 `passed`：3102 fixed steps、9 个 retained frame sample、零 diagnostic；该结果只覆盖 media/provider 与标题恢复接线，不覆盖完整剧情 terminal。
- [ ] 正式音频人工听审、完整路线与自然 unlock、gallery、Linux FUSE、macOS extract、Manager 实机预览和 Windows E3 仍未闭合；旧 cache second-run 不属于当前 key-file/streaming identity，不能替代这些门禁。

## 当前媒体复核（2026-08-26）

- rebase 到最新 ABI v9 `master` 后，签名动态 plugin 重新绑定 AstraMedia `ffmpeg-vcpkg`，真实授权样本 Headless slice 通过 139 fixed ticks、5 个 retained checkpoint samples、638 次有界 VFS read 和零 diagnostic；`title_initial`、`config`、`movie_60` 三张图已人工查看。该证据只关闭当前 media/provider 接线回归，不关闭完整路线、正式性能或 Windows E3。
- 同一 FFmpeg release slice 在 CLI retained `Layer2D` composite cache 变更后重新执行：139 fixed steps、5 个 retained samples、638 次 bounded VFS read、zero diagnostic；`step_total` 从约 81.9 s 降到约 35.9 s，`effect_dispatch` 从约 48.7 s 降到约 22.4 s。缓存只在 viewport、完整 layer state 和已验证 base frame 全部相同时生效；该结果是性能诊断，不替代正式 120 Hz 或完整路线门禁。
- FFmpeg feature 未编译、profile 没有 `ffmpeg-vcpkg` binding、AVI identity 不符或 codec/container 不受支持时，Minori 直接返回 blocking diagnostic；不存在 WMF、RFVP、手写 AVI/WMV 或按注册顺序选择的 fallback。

Minori family-mounted image previews now use explicit `astra-media` `DecodeProviderRegistry` bindings: standard PNG/JPEG/BMP/WebP use `astra.decode.image`, and ANI/SQZ use the family-owned `astra.decode.minori.image` provider for a bounded first frame. UI receives RGBA8 pixels, not paths or native handles. Family audio previews use an explicit Symphonia binding and expose metadata only. Minori AVI playback and preview bind AstraMedia's incremental `ffmpeg-vcpkg` provider; an absent or mismatched binding remains blocking.

Manager startup no longer eagerly loads the unselected FVP binary. The composition root creates the pure-Rust Minori idle provider and rebuilds the selected family only after a validated mount is available; same-family rebuild is intentional because the idle and mounted VFS bindings differ.

本页较早记录的 route 数字来自 ABI v9 之前的 local-private run；在当前 `635527831e89e5ff9b87ac165b5b5532e28356c6` consumer 身份上重新生成完整四路线、自然解锁和 Windows E3 evidence 前，不把它们当作当前发布证据。

## 当前状态（2026-08-25）

- 本轮已将 runtime snapshot schema 硬切到 `astra.emu.minori.runtime_state.v24`；message completion 会记录排序 bounded read identity，且 restore 会拒绝重复、无序或超限记录。此前 v23 的历史描述仅用于回溯，不能作为当前 ABI/状态版本。

- Manager VFS preview 已增加 UTF-8/UTF-16 BOM 与 legacy CP932 的有界编码检测，并在 UI 显示实际编码；不符合文本编码的内容继续进入 hex 视图。Manager 也已按显式 family 接入 Minori launch profile、`LegacyMountedVfsReaderAdapter` 和静态 runtime provider，family-mounted tree/文本 preview 读取解密 URI；PNG/JPEG/BMP/WebP、ANI/SQZ 首帧、音频 metadata 和 Minori AVI video 的 provider binding 已落地，非 AVI Minori video 会在 family 边界直接阻断。真实 Manager 窗口预览和 Windows E3 仍未完成；Headless media slice 已形成独立 E2 证据。

- 当前 consumer 分支直接 rebase 到 ABI v9 基线 `635527831e89e5ff9b87ac165b5b5532e28356c6`，没有保留 v7/v8 兼容层。Minori 已在 `Native + MultiLayer` 主路径接通 VFS、可写 surface、同步 translation Hook、CosmicText text layer 和 writable-file save/global-progress port；旧 scene、snapshot、text lease、session-resource 与 provider-result API 只返回 blocking diagnostic。
- 当前签名 package 在真实八包上完成首路线、Config、backlog、save/load 和 local-private gallery 增量 E2。首路线报告为 `25499` fixed step、`13170` presented frame、`25` 条输入、`7441` coverage id、terminal true、零 diagnostic；gallery 复验为 `82` fixed step、`12` frame、`64` 条输入、9 个 checkpoint、零 diagnostic。`cgthumb` 已按真实 `128x72` 尺寸严格校验。
- 视觉检查已覆盖标题、Memories、BGM、CG、回想、Config、backlog 和首路线选定 checkpoint；movie gallery 仍只有已验证 Memories 背景 + 有界文字层的严格近似，不能写成原版 parity。local-private global progress 不能证明四条路线自然解锁；正式人工音频 review、完整四路线和 Windows E3 仍未闭合。

- 分支已同步到 Family ABI v9 基线。Minori 必须迁移为 `Native + MultiLayer`，并使用 Host-owned surface、同步 Hook 和 writable-file port；旧 scene、snapshot、text lease、session resource 与 budget API 已删除且不提供兼容层。
- 历史 2026-08-23 ABI v9 迁移条目保留为过程记录；当前同步 Hook、CosmicText text surface 和真实 v9 E2 已由 2026-08-25 条目覆盖。
- 授权样本仍是 8 个逻辑 archive、18 个物理 PAZ 文件和 14502 个 entry。`mov` role 含 5 个 RIFF/AVI；视频为 WMV3 1280×720、24 fps，音频为 PCM 48 kHz 双声道 16-bit。当前生产路径由 AstraMedia/FFmpeg 负责 demux、codec、resample 和 packet timing；Minori 不再依赖纯 Rust WMV/AVI decoder。真实 Headless media slice 已验证 60 fixed ticks、16 帧、111104 音频帧、非静音和零 diagnostic。
- 签名 Minori dylib 已用同一 mount、plugin、Headless profile 和序列化物理输入连续跑完两次标题启动的真实路线。两次均推进 31011 fixed steps、提交并栅格化 31627 帧、消费 16947 条输入并最终从标题执行 Exit；snapshot round-trip、用户 save/restore、Config、backlog、真实影片、自然解锁和 31 个 checkpoint 均通过，diagnostic 为 0。新的同身份单次运行进一步推进 33490 fixed steps、呈现 34108 帧、消费 16951 条输入并通过 33 个 checkpoint，把首个 choice、实际 post-choice 分支和自然完成的不可跳过结局媒体纳入同一份通过报告。
- 两次运行的 visual trace、runtime state trace、route terminal、coverage、audio meter、submitted scene、rasterized frame 和 audio stream hash 全部一致。输入 hash 因 local-private session id 不同而不同，不作为跨 session 一致性结论。平台全局进度通过 ordered storage request/result 原子读写；两次均严格证明自然解锁数为 1。restore 会合并同一 provider session 已确认的全局进度，不允许旧 snapshot 回滚解锁。
- 模型复核新报告全部 33 个 required checkpoint；人物、背景、影片和日文字形没有缺失、横向裁剪、非预期拉伸、旧图层残留或未退场人物。choice 与 post-choice 已从独立真实 slice 升级为同一条完整路线证据。
- 公共 Kira limiter 后 output peak 为 0.989551，output overload 与 underflow 均为 0。自动 E2 已闭合这条路线的结局返回标题、最终 Exit、snapshot continuation、用户 save/restore、自然解锁和 VM/视觉/音频重复运行确定性。原版 `Memories` 菜单只在第四条已确认路线后出现，因此首条路线不能形成 CG/BGM/回想 checkpoint；鉴赏完整入口、Config 行为级 E2 和 Windows E3 仍开放。当前状态不是完整产品体验或 E3。
- Config 已按原程序 29 类 action 建立 v23 draft transaction、精确鼠标命中区、滑块换算、四类 retained texture overlay、WAV 试听和公共音量/静音映射。真实八包短程 Headless 已以物理 pointer 验证画面效果开关、BGM 滑块和 `BGMTest.wav` 试听；文字阴影另以 384 fixed steps、388 帧和 34 条输入验证关闭选项后剧情消息使用无 outline 的公共 typed text path。相关 checkpoint 的模型视觉检查通过。全屏 Host effect、逐字速度、其余视觉开关对剧情的实际影响和角色语音筛选仍待验证。
- Config 的已应用值现在在显式 `astra.provider.storage = astra.writable_file.v1` 时按 `astra.emu.minori.config.v1` 保存到相对 writable-file root；读取严格绑定 case/package/profile identity，缺失文件使用原程序默认值，损坏、越界或 identity 漂移直接阻断。Save slot restore 保留当前 installation-scoped config，不会被旧 gameplay slot 静默覆盖；这关闭了配置跨 session 的持久化契约，但不替代全屏 Host effect、逐字速度和其余行为级证据。
- runtime state v19 已在当时的 release plugin 上重跑同一输入序列：33490 fixed steps、34108 个呈现帧、16951 条物理输入和 33 个 required checkpoint 均通过。当前代码已硬切到 state v23；三态 play mode、Auto 入口、message voice 与 backlog voice replay 均有定向回归。活动消息现以同 token 的 `Input`/`Time` modality 重绑定响应 Auto 切换。当前签名 Release plugin 已在真实八包上用最快 Auto 持续跑完首路线：25552 fixed steps、25557 个 GPU frame、50 条物理输入、terminal、snapshot round-trip 和零 diagnostic 均通过，正文阶段没有周期性 Enter。v21 完整路线另以 33553 fixed steps、34172 帧、16957 条输入和 34 个 checkpoint 验证物理 Enter 的 backlog voice replay 不推进 VM、保留同一 message wait，并继续到 terminal。完整 WAV 的具名人工听审未完成，因此正式 review 保持 blocking。

### Control 快进增量（2026-08-03）

- `.pragma enable_control`/`.pragma disable_control` 与 `.pragma skip_enable`/`.pragma skip_disable` 已作为两组独立 gate 进入严格 runtime；Control pressed/released edge 与 `Normal/Auto/Skip` 互斥 play mode 绑定 session，并进入 runtime state v20 snapshot。
- 只有脚本同时允许 Control 和 skip、且物理 Control 正被按住时才跳过已确认的 `.wait` 时间命令；Host 仍逐 tick 推进，message、media/presentation fence、provider completion 和未知 pragma 继续阻断或等待。
- 同一签名动态 plugin 的私有 Headless control sequence 完成 300 fixed steps、54 个呈现帧、9 条输入、snapshot round-trip 和非静音音频，diagnostic 为 0；仍未 terminal。
- focused runtime/provider tests 已通过；完整路线、演出、影片 codec 和 Windows E3 仍未闭合。

## Rebase 状态（2026-08-03）

本页的历史 E2 数字不覆盖本次 rebase 后复核。当前可复现的纯 Rust Headless slice 为 481 fixed steps、24 个呈现帧、27 条输入消息、snapshot round-trip 成立、diagnostic 为 0，并产生非静音音频 artifact；入口没有到达 terminal。视觉复核确认启动标题帧和末帧非空，但中段 checkpoint 尚未显示可读消息，因此不能把该 slice 写成“正文已验证”。`Firefly`、选择项和 `.effect2 SnowH` 已有局部实现与测试；它们尚未形成同一条真实 v8 Headless 路线证据。未确认的 effect、movie codec、系统页和完整路线继续保持 blocking。

## 当前实现与证据

| 项目 | 状态 | 证据边界 |
| --- | --- | --- |
| `family-core` mount/read_dir/stat/read_range/open_stream 契约与 manifest v2 | 已实现 | unit/compile；`family-api` 已硬迁移为 ABI DTO，不保留 VFS re-export |
| PAZ v0-v2、分卷、zlib、随机读取 | 已实现 | GARbro contract + synthetic tests；真实八包 14502 个 entry 完成 decoded full verify |
| Minori family-owned 流式解密 | 已实现 | Blowfish、RC4 skip、archive XOR、zlib、movie transform；没有 Luau callback、明文 cache 或 fallback |
| 严格 `key.toml` | 已实现 | 有界私有文件读取、八 role、hex/Blowfish 长度、CP932 和 movie key 约束已有 unit tests |
| 公共 viewer tree/stat/page/search/text/hex/media binding | backend 已实现 | image/audio/video 必须显式 `DecodeProviderRegistry` binding；Manager UI 接线和真实预览验收待补 |
| 公共 desktop verify/extract | 已实现 | Windows 八包 manifest v2 full verify 已通过；extract contract 已接入，macOS 运行证据待补 |
| Linux foreground read-only FUSE | 代码已接入 | 缺真实 Linux FUSE 证据，不标完成 |
| `.sc` CP932 lossless IR、CFG、unknown command、census | 已实现 | 89 文件/33728 行/33695 command/29 token，unknown opcode 0；`census-scripts` v5 另输出不含 URI/正文/operand 的逐文件序号、源 hash、大小和 opcode 计数；`select` 的 display/label pair、选择移动和跳转已进入严格 runtime |
| ANI/SQZ container 与 `bg`/`bgm` census | adapter 已实现 | 2655 PNG、1951 ANI/6723 frames、9 SQZ/224 frames、49 Ogg 真实读取通过；渲染/播放尚未验收 |
| Minori deterministic VM state 与 control-flow | E2 route | 已覆盖 chain/call、label/goto/if、变量、message/select/wait、stage/character/panel、CrossFade2、Firefly、axis scroll/ScrollXF/WScroll2、BGM/SE/voice/movie 和 end；未确认 operand 继续阻断 |
| Minori runtime provider / `cdylib` ABI | E2 增量 | Family ABI v9 已 hard cut；共享 Host adapter、writable-file、资源/文字 surface、同步 Hook 与四层 `Native + MultiLayer` 已接通，受影响 library/CLI/Manager tests 通过。仍缺完整四路线与正式 Windows host evidence |
| Minori 演出、系统 UI、完整模拟 | E2 增量 | 首路线、Config、backlog、save/load、choice、post-choice、影片和 local-private gallery checkpoint 已有增量 evidence；movie gallery 背景是严格有界近似，完整自然 unlock、正式 audio review、原版 gallery parity 与 Windows E3 仍开放 |
| Config writable-file persistence | 已实现 | `astra.emu.minori.config.v1` identity-bound envelope、原子替换、默认值与损坏/漂移阻断有定向测试；完整跨进程桌面复验仍待补 |

当前合法样本包含八个非空逻辑 archive 和 18 个物理文件。key-file/streaming identity 已完成 14,502 个 entry、43,818 个逻辑读取范围和 6,624,958,365 个 decoded bytes；89 个脚本的 payload-free census 也已在同一 identity 下通过。Linux FUSE、macOS extract、Manager media preview 和 VM 仍各自保留独立证据边界。

## 下一阶段 Archive

- [x] Probe game root and classify `bg/bgm/scr/st/sys/se/voice/mov`，包括 `bg.pazA` 至 `bg.pazJ`。
- [x] 严格解析八 role `key.toml` 并在 mount 时一次性读取。
- [ ] 对八包执行 key-file/streaming identity 的 decoded full verify；旧 no-cache 结果不作为本项通过证据。
- [x] 对每个 entry descriptor 校验 offset、packed size、unpacked size 和 method。
- [x] 拒绝 path traversal 和绝对路径 entry。

## Script

- [x] 从 `scr.paz` 与原程序候选确认入口文件 `test.sc`；完整稳定 URI 由 launch profile 的 `runtime.entry_uri` 唯一指定，CLI 不接受 `--entry` 覆盖。
- [x] 拆分 select、普通 voice、stand 与本轮路线用到的主要演出 operand；未知形态仍按 source span/raw operand 阻断。
- [x] 未确认 command/operand 保留 raw bytes、source span 和 `Unknown`。
- [x] 完成全部已确认资源引用映射；BGM/SE、message voice、movie、stage 前景/背景、stand、CrossFade2、Firefly、SnowH、panel、chain script 均复用 VM 的已验证 operand grammar。显式 `astra.resource_audit=full`（CLI `--audit-all-resources`）会先有界枚举所有 `.sc`、读取并解析引用，再逐项 stat 非空资源；缺失、短读、超限或 VFS 不支持枚举均阻断。未知 opcode/effect 仍不猜测，按执行路径阻断。

## Runtime

- backlog voice replay 现在尊重已应用的 `backlog_voice_playback` 开关：关闭时不提交新的播放命令，但保留 backlog 的 voice identity；角色 voice filter 与该 preference 都有 VM regression。全屏 Host effect、逐字速度和正式音频听审仍未关闭。

- [x] boot 到首个 message；正文经一次性 lease、CosmicText 和 Renderer2D 形成真实 checkpoint，未进入 snapshot/report。
- [x] 物理 Enter 推进与受 pragma 门控的 Control 快进可跑完首条剧情路线；Control 现在也会把已显示消息的同 token `Input` wait 重绑定为 10 ms `Time`，真实八包以 25496 fixed steps、25498 帧和 28 条输入跑完首路线，正文阶段没有周期性 Enter。backlog 的当前记录、滚轮导航、关闭恢复和当前记录 voice replay 已进入真实完整路线 E2；额外短程以 771 fixed steps、776 帧和 55 条输入验证两次上翻显示不同历史记录，idle tick 不再清空 retained text。Auto/Skip 三态、原版菜单命中区和等待重绑定已完成定向测试；真实八包分别在最快 Auto 与持久 Skip 下跑完首路线。
- [x] Config 的 29 类已确认 action、Apply/Cancel transaction、pointer drag、状态 overlay、WAV 试听、音量/静音映射和 snapshot round-trip 已完成 E1。
- [x] 真实 Config 资源完成 Headless 物理 pointer、screen-effect checkmark、BGM knob、WAV 试听和视觉审查；短程运行自动通过，正式 review 因完整 WAV 未人工试听而按协议阻断。
- [ ] 接通全屏 Host effect、消息逐字速度、画面效果/动画效果对剧情演出的实际影响和角色语音筛选；文字阴影已接通 typed outline 并通过定向 E2，缺其余任一行为仍不得标完整 Config。
- [x] choice 选择状态、label 跳转、三态资源、批量 option lease、居中排版和提交清除已进入 VM/provider/Host；真实脚本以严格 `minori.choice_active` observation 捕获 choice 与 post-choice checkpoint。
- [x] provider snapshot restore 会重新绑定当前 system page、message 或 choice presentation，并清除恢复前未消费的一次性文字 lease；首个 restore output 会在需要时把 retained scene 与新文字放入同一 typed transaction，完整路线 user save/restore 已验证该 continuation。
- [x] 显式 checkpoint 在捕获前提交待处理 Scene2D；Config 与 backlog 打开/关闭已由真实短程画面变化验证。
- [x] 用户 save/load 后 continuation 成立；两次完整路线的 visual/runtime/terminal/coverage/audio hash 一致。
- [x] runtime 只接受原程序已确认的四个 global clear flag，并把脱敏 unlock identity 纳入 snapshot；provider 会报告 session 内 unlock count，未知 `CLEAR` 名称不参与解锁。
- [x] global clear flag 经 ordered platform storage 原子提交并在 provider session 恢复；真实路线严格断言自然解锁数为 1，title variant 只接受四个已确认 flag。
- [x] family snapshot 固定携带独立的 `astra.emu.minori.global_progress_snapshot.v1` section；合法的 unloaded 静止态可供 restore rollback 保存，实际 pending storage I/O 仍严格阻断。

## Media

- [x] 背景、立绘和系统 UI 使用 retained Scene2D 分层输出；立绘 `load/pos/keep/vis/trans` 已接入固定 tick 透明度动画、等待和 snapshot。合成脚本的 256→128→0 透明度序列已通过 Headless WGPU capture 和人工视觉检查。真实样本没有 `trans/vis`，因此该证据只关闭合成动态立绘 E2，不计入真实路线 coverage。
- [x] BGM、SE、message voice 分通道；message voice 已按 IDA `resource[volume,pan]` 合同和 7,047 个真实 identity 全量绑定到 `voice.paz`，并通过公共 Ogg audio command 发出。完整首路线 Headless E2 已覆盖该路径；具名人工整段听审仍开放。
- [x] backlog 当前记录的 voice replay 由原程序 Enter 路径确认；runtime 重播 stream 4 且保留原 message await，不推进 VM。真实 Headless 物理 Enter、checkpoint、后续 continuation 和 terminal 已通过；具名人工听审仍开放。
- [x] `mov.paz` 的 5 个 RIFF/AVI 由 range-backed AstraMedia `ffmpeg-vcpkg` incremental provider 播放；缺 provider、格式漂移、短读和 fence 异常直接阻断。
- [x] Minori AVI provider 在解析前拒绝空/超过 64 MiB 的预览输入，并校验 WMV3 尺寸、单包和 decoded RGBA 帧预算；边界测试 4/4 通过。该项不扩大 codec 覆盖或 movie parity 证据。
- [x] decoded video 通过公共 `SceneCommand::VideoFrame` 合成；movie skip 只接受脚本明确标记为 skippable 的分支，并等待 Host 完成原 fence。
- [x] 公共 Kira main track 使用显式 peak limiter，分别报告 pre-master 与 master-output；真实短程 output overload 和 underflow 均为 0。
- [x] 真实 movie frame checkpoint 已确认 decoded frame、比例、剧情层文字合成和无旧帧残留；完整路线 WAV 非静音、低于 i16 full scale，master output overload/underflow 为 0。
- [ ] 完成正式 Headless audio review；视觉 bundle 已逐项检查，WAV 的格式、时长、peak、RMS、静音区间、clipping 和声道平衡已量测，但涉及语音的整段试听尚未完成，`validate-review` 保持 blocking。
- [x] review protocol 强制 `full_audio` verdict 与 bundle 的完整 WAV selection；省略或失败必须在 validator 和 release preflight 阻断。私有 10 段连续听审清单覆盖全部 27260416 audio frame，但尚未据此宣称人工听审完成。

## 后台进度与窗口焦点（2026-08-25）

- `progress_in_background` 已进入 Minori provider 的有界 blackboard observation；默认关闭不发首 tick 边沿，持久化开启和关闭切换、load/restore 都会重新建立可消费状态。
- Windows native host 只对 Minori 消费该 observation：失焦时按配置暂停或继续固定 tick/音频，其他 family 不改变原焦点行为；非法 observation 值直接阻断。
- provider/CLI 定向测试通过；真实窗口焦点、音频恢复和完整路线仍需 Windows E3，不能把单元测试记为平台验收。

## 2026-08-25 当前 v9 复核

- [x] 重新安装 stable Rust 1.98 后，以当前 signer/trust-root 重新编译、签名并运行 Minori v9 短程；431 fixed step、27 条物理输入、13 个 submitted/rasterized frame、344576 个 audio frame、无 diagnostic，标题与场景 frame hash 不同。
- [ ] 旧 gallery 输入在当前 mount/profile/global-progress identity 下触发 `ASTRA_EMU_HEADLESS_CHECKPOINT_AFTER_TERMINAL`，不能沿用历史 gallery report；需要重新生成匹配当前状态的物理输入并完成 required checkpoint 复核。
- [ ] 四条自然路线、自然 gallery unlock、正式 audio review、movie gallery 原版视觉 parity、cache second-run、Linux FUSE、macOS extract 和 Windows E3 仍未关闭。

## 2026-08-26 稀疏采样回归

- [x] checkpoint 捕获前物化 pending retained scene / prepared CPU layer；不推进 fixed tick，不生成替代帧。
- [x] `frame_sample_interval=60` 的真实八包短程通过：431 fixed steps、7 frames、27 条输入、零 diagnostic；该结果不替代完整路线和平台证据。

## Release Gate

- [ ] 本地 case report 只包含 hash、coverage、diagnostics 和命令。
- [ ] 不包含 payload、截图、音频、视频、完整脚本或 key。
- [x] 完成 Minori clean Release 120 Hz GPU performance run。同一 build、package、profile、输入、adapter 与 driver identity 连续三次完成 1200 帧 warmup 和 72000 帧十分钟测量；deadline miss、audio underflow、full resync、trace dropped、稳定段 upload/readback/allocation 与 memory growth 均为 0，runtime p99 为 0.2725–0.2938 ms，presentation p99 为 0.81456–1.11064 ms。该静态标题负载不替代完整路线、影片/音频或 Windows E3。

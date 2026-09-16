# 全产品重构实施状态

基线：28f89d88。用户于 2026-09-12 确认重构范围，当前状态按实际代码与运行结果维护；历史 Stage 状态不代表新架构完成。

| 阶段 | 状态 | 尚需完成 |
| --- | --- | --- |
| 0 规则与测试 | 进行中 | 新宪章/契约与轻量文档检查已落地；EMU 独立 workspace 和 xtask 已接通；541 处普通测试已迁移，强制 Headless 宏与旧状态矩阵已删除 |
| 1 EMU 薄 API/FVP | 进行中 | typed 配置、可选翻译、驻留库与 FVP 编码适配已整合；待整合验证和真实游戏运行 |
| 2 SDK/Minori | 未完成 | 独立 astra-text、Minori archive/profile、Family session、实际视听与自有存档已整合；完整 opcode、动画/长媒体和真实游戏验收待完成 |
| 3 跨平台 EMU | 未完成 | 三桌面/Android Manager、核心与真实媒体运行 |
| 4 Engine/VN | 未完成 | 演出 tick 去除整会话克隆并修复排队存档；无包 World 与 typed Actor/Component 存档已接通，Runtime 整帧回滚、通用 replay 与历史 hash chain 已删除；其余任务、可信 Luau、typed 产品主路径和 DSL 待完成 |
| 5 Editor/Agent | 未完成 | GPUI、文本/图/时间线、独立预览、ACP/MCP 与两种编辑模式 |
| 6 终之空 | 未完成 | 新 .astra 工程、Classic/Modern 37 路线及私有四平台包 |
| 7 整体验收 | 未完成 | 全活动产品检查、真实流程、固定场景性能与旧路径清理 |

## 验收安排

用户于 2026-09-15 指定后续重构全部由主线程实施，不再使用子智能体；总体范围、阶段顺序和验收目标保持不变。

本环境优先做可执行的实现和自动测试；另一环境中的合法游戏源和设备于阶段验收时接入。Windows 承担终之空 37 路线和两款 EMU 各一结局；其他三平台执行代表流程。未接入设备的结果保持未验收。

性能目标为 VN 桌面 1440p120、移动 1080p60。记录实际设备与固定场景结果，EMU 按原生速率分别测核心与 Host。

用户补充设备范围为“主流配置”；CPU/GPU、内存、macOS 机型与 Android SoC 尚未指定，正式测量时记录实际型号，不能把“主流配置”当成已固定的性能基线。尽量通过远程环境完成工作；当前环境不可用的商业游戏与转换工程在另一环境可用，具体接入方式和设备远程权限待提供。

用户指定 OpenAI Responses/Completions 为优先接入方向。它是模型 API 选择，尚不能确定是外部 Agent 的后端，还是需要项目直接实现 Agent 循环；该取舍待澄清，当前 ACP/MCP 契约继续有效。

## 当前验证记录

- 独占重构 worktree；并行子任务各用独立 worktree/target。
- 新文档检查 222 页通过；7 个链接/卫生回归测试通过。
- EMU 独立 workspace 全量 fmt/clippy/build/test 通过：169 项测试通过，3 项按 GPU/联网条件未执行；含 API v2、Manager、FVP 和 Minori CLI。
- xtask 与 Linux platform all-target clippy 通过；修复 default build 的 audio fault-injection feature 组合错误。
- 普通逻辑测试与独立文字库已整合：子任务验证包含 104 个普通测试、15 个独立文本测试和 5 个媒体适配测试。
- Engine 全量 fmt/clippy/build/test 复验通过：666 项测试通过、0 失败、9 项按原条件未执行。修复了显式 Headless fixture 生命周期、无效 timeline fixture 和日志队列时序测试；已删除旧矩阵检查。
- 演出改动子任务的 35 项测试、all-target clippy 和 Player VN 调用方编译通过；Minori archive/profile 子任务的 40 项测试与 clippy 通过。
- Minori session 整合后 core/CLI fmt/clippy/build/test 通过，45 + 10 项测试通过。公开 fixture 覆盖 PAZ/SC、实际画面、PCM、输入、存读档和取消/关闭；动态库构建由子任务验证。尚未支持的 opcode、ANI/SQZ session 播放、长媒体流式解码和 Android 注册保持未完成。
- Minori 音频渐变现进入 AMINSV02 自有存档，保存采样进度并按剩余时间恢复；实际 PCM 连续性、fade-out 停止边界、参数替换和未播放资源停止测试通过。最新 Minori/CLI 全 feature 50 + 10 项测试、clippy 和 fmt 通过；旧 AMINSV01 拒绝且不覆盖。
- RuntimeWorld 构造不再要求 package；产品身份由宿主首 tick 前显式附加，无包/无 FSM 的 typed 更新、tick 和保存恢复已验证。Runtime world v5 与 NativeVN save_blob v5 同步，旧格式拒绝。Engine 最新全量 fmt/clippy/build/test 通过，669 项测试通过、9 项按原条件未执行。
- RuntimeWorld 整帧事务 checkpoint 和五类撤销日志已删除；执行错误/动作 panic 会终止 World，预检错误仍可重试，保存和可变 API 显式传播错误。部分提交、写入/快照拒绝、损坏存档及成功恢复均通过测试。Engine 最新全量 fmt/clippy/build/test 通过：670 项通过、9 项按原条件未执行。
- 真实 GPU/商业游戏/其他平台验收仍未执行。
- NativeVN 恢复先验证候选 World 和 typed VN state，再一起提交；无效状态保留当前会话与待处理控制，成功恢复清空旧控制。修复 save_blob v5 的外层数字版本不一致，新增 hash/版本/包身份检查。合法 hash 下的缺失/重复组件、错误版本、错误 typed payload/schema 和拒绝恢复后的失败状态已验证；Engine 全量 fmt/clippy/build/test 通过，673 项通过、0 失败、9 项按原条件未执行。

- Runtime 通用 replay recorder/transcript/checkpoint、Replay tick mode、HistoryChain 与 aggregate state/event/presentation 摘要 API 已删除；LoadReport 只返回 step/seed。保存恢复与并行调度测试改为比较实际 snapshot/完整存档字节，两种模式的 tick 均验证不编码 typed component。Runtime/VN 保存针对性测试 43 项通过，Engine 最终全量 fmt/clippy/build/test 通过：672 项通过、0 失败、9 项按原条件未执行；诊断记录存储和 Shipping/Evidence 检查差异仍待后续统一。

- Runtime 完成句柄绑定 host 内 TaskScope，成功读档、失败和销毁使旧工作失效；子作用域取消及单 token 取消移除排队结果并发出取消事件。同一 token 只接受一个终态，重复/无主完成存档拒绝；旧 AwaitReplayPolicy 已更名为 AwaitCompletionPolicy，文本字段同步而 Postcard 布局不变。NativeVN 同步 wait 消费改用句柄，未使用的 provider 局部 save_slot/load_slot 删除。真实 worker 迟到、跨 World、取消、恢复和重复终态等 Runtime/provider 针对性测试 52 项通过，Engine 全量 fmt/clippy/build/test 通过：680 项通过、0 失败、9 项按原条件未执行；通用任务组合器、可信 Luau 与产品异步 IO 的完整接入仍未完成。

- 演出 coordinator 共用 fence 改为 all-of 等待，覆盖活动与排队成员；文字点击、单视频结束不提前完成，失败/替换保持组 Failed，其他轨道继续。成员身份冲突在提交前拒绝，终结后的空组可重新使用。coordinator v5 拒绝旧 v4 和不一致的恢复状态；ProductStageDirector 双视频层等待已验证。Engine 全量 fmt/clippy/build/test 通过：688 项通过、0 失败、9 项按原条件未执行。真实媒体播放、通用任务组合与异步 IO 接入仍未完成。

- Player 呈现帧已改为原地推进，移除 StageDirector、stage state、旧 scene draw 和活动转场 source snapshot 的逐帧回滚克隆。执行失败或当前等待的 fence 失败会终止呈现会话；损坏存档保持失败，成功恢复才重开。读档首帧重建已恢复场景及纹理生命周期。新增两项真实 package/字体/图片 fixture 回归覆盖帧后资源失败、输入/保存拒绝、恢复前后失败和无关 fence 隔离，均通过；本轮 Engine 全量 fmt/clippy/build/test 通过：690 项通过、0 失败、9 项按原条件未执行；真实硬件验收未执行。

- Player 视频请求绑定 host-owned TaskScope；同层替换、读档提交、呈现失败和退出取消旧请求，decode/frame/fence 入口拒绝旧作用域。恢复提交清空旧媒体/timeline/stage completion 和 UI 请求，验证失败保留队列；Media Host 恢复保留旧流关闭任务，成功关闭后才移除。新增真实 worker 迟到、替换/外来作用域/退出及连续恢复关闭队列回归；Engine 全量 fmt/clippy/build/test 通过：692 项通过、0 失败、9 项按原条件未执行。随后补充取消视频不消费继续输入的回归，并统一测试夹具模块；最终 Player VN fmt/clippy/build/test 复验 65 项通过、0 失败。通用任务组合、完整音频/timeline 作用域与真实媒体验收仍未完成。

- 原生 Player 两条入口与 Headless 已统一产品存读档 API，保存音频/视频/timeline，恢复前检查缺失媒体、schema、timeline 和音频格式；删除分散的“取出媒体 JSON 后另行恢复”接口。native session 补齐捕获与保存元数据。两项 package 回归验证媒体恢复和预检失败不提交剧情；Engine 全量 fmt/clippy/build/test 通过：695 项通过、0 失败、9 项按原条件未执行。播放时钟重新定位、冷启动声音资产恢复和完整产品长流程仍需后续验证。

- Media Host 新增独立播放时钟，保存 `playback_time_ms`，恢复后首次调用重新绑定宿主时钟并保留剩余时长；视频起点与 timeline deadline 共用该播放时间。媒体 snapshot v3 拒绝旧 v2 与未来时间字段，未重开的恢复视频也进入再次保存。Player VN 35 项单元测试通过，覆盖新/旧宿主时间原点、时钟错误不推进、视频立即重存和错误 snapshot 不提交。视频流集成回归补充长暂停后保持第一帧、到剩余 PTS 才呈现第二帧的断言；Engine 全量 fmt/clippy/build/test 通过：698 项通过、0 失败、9 项按原条件未执行。冷启动音频资产恢复、解码实际恢复播放与真实设备长流程仍未验收。

- 视频帧绑定现在重建携带当前像素的 `VideoFrame` 场景命令，场景刷新借用当前 StageDirector；避免沿用旧 frame，帧执行错误终止呈现会话。恢复解码到保存 cursor 后重新提交该帧，解码/校验/呈现错误保留关闭任务。视频与音频 Host 5 项集成测试通过，新增尺寸变化与像素断言，恢复流程直接核对第一帧和后续帧的 BGRA→RGBA 字节；Engine 全量 fmt/clippy/build/test 通过：699 项通过、0 失败、9 项按原条件未执行。此处验证平台命令与解码 fixture，不替代真实 GPU/商业视频播放验收。

- AudioServiceSession 新增恢复预检，停止当前声音前检查已准备 PCM、cursor、voice/bus 容量、序列与完整渐变状态。Player 已打开音频服务时复用该检查，未打开却包含声音/渐变的恢复也提前拒绝，避免剧情先提交。7 类无效 snapshot 的拒绝与 live timeline 保持不变已验证，astra-audio-kira 4 项测试通过；Engine 全量 fmt/clippy/build/test 通过：700 项通过、0 失败、9 项按原条件未执行。冷启动 package 音频解码/资产准备与真实设备恢复仍未完成。

- 产品恢复入口改为 async，在提交剧情前按当前 package 和显式 decoder 准备缺失 PCM；正常播放与恢复共用 canonical PCM 准备，检查真实服务缓存避免陈旧 prepared 标记。原生 Player 与 Headless 已同步调用。冷启动媒体 Host 的 package WAV 回归通过：解码、cursor/paused/bus 恢复、重复恢复复用 PCM、外来 package 与错误 PCM 长度拒绝均已验证；Engine 全量 fmt/clippy/build/test 通过：701 项通过、0 失败、9 项按原条件未执行。实际 decoder/设备组合、进程重启长流程和输出队列恢复仍需真实环境验收。

- 产品读档在媒体状态恢复后停止旧 Kira worker、关闭旧音频端点并重建所选 output，复用共享 PCM 与存档 timeline。端点关闭失败保留 handle，打开后的格式/初始化失败也保留清理所有权；未清理 handle 阻止再次打开，退出清空待恢复 PCM。集成回归验证两次读档的关闭/打开顺序、PCM 复用及关闭失败后会话拒绝 tick、退出重试清理。Engine 全量 fmt/clippy/build/test 通过：701 项通过、0 失败、9 项按原条件未执行；真实设备缓冲与听感连续性未验收。

- TaskScope 取消实现复用已锁定的 tokio-util CancellationToken，新增 cancelled 等待与 run 的 typed Completed/Failed/Cancelled 结果。普通 async 顺序短路和 futures try_join 并行组合复用同一作用域，不新增调度器或存档类型。8 项异步作用域测试及既有 8 项完成句柄测试通过，包含并行整组取消释放所有分支回归；Engine 全量 fmt/clippy/build/test 通过：709 项通过、0 失败、9 项按原条件未执行。平台资源的异步关闭、可信 Luau 与产品任务组合接入仍未完成。

- 视频启动/关闭从 Media Host 大文件拆成独立模块。打开成功立即登记待关闭 session，描述和 decode 成功后才转交活动视频；失败尝试关闭，关闭失败或启动 future 被 drop 均保留清理责任。两项集成回归通过，覆盖无效描述、错误输出类型、解码失败、关闭重试和迟到响应拒绝；Engine 全量 fmt/clippy/build/test 通过：711 项通过、0 失败、9 项按原条件未执行。平台 open 自身被中断的资源回收和真实 decoder/设备验收仍未完成。

- Player PlatformCommandSink 持有未交付 decoder open/close future，待处理上限 64，逻辑 id 冲突在发送前拒绝。新增 cleanup_pending_decode_opens，保留中断的清理响应与失败关闭重试，has_live_resources 包含未交付资源；Media Host shutdown 已接入。3 项命令层测试与通过 TaskScope 取消的媒体退出回收集成测试通过；Engine 全量 fmt/clippy/build/test 通过：715 项通过、0 失败、9 项按原条件未执行。直接 PlatformHostClient 与其他资源种类的 open 取消仍未处理，真实平台生命周期尚待验收。

- NativeVnProductAudioHost 持有唯一 pending open/close future，取消后可继续同一响应；shutdown 对未交付端点只接收并关闭，不启动 mixer。恢复与退出共用可中断后继续的关闭路径，明确关闭失败保留重试所有权。端点 lifecycle 与 snapshot 拆成模块。两项回归通过，覆盖 TaskScope 取消、shutdown 中断、lane 释放且未启动 worker、格式错误与关闭重试；Engine 全量 fmt/clippy/build/test 于 2026-09-15 重新完整执行并通过：717 项通过、0 失败、9 项按原条件未执行。直接平台客户端的其他资源取消和真实设备退出/恢复仍未验收。

- NativeVN Player 改为私有 NativeVnRuntimeHost 直接持有具体 NativeVnRuntimeProvider，移除生产调用链中的 ProductRuntimeHost、同步/异步桥、mailbox、通用 worker 与外层 mutex。复用共享输出/section 校验，保留 binding、单 session、step/seed/mode 和失败状态；合法恢复可恢复执行，损坏恢复不覆盖现态。35 项既有单元测试和 2 项 binding/生命周期/恢复/预算回归通过；Engine 全量 fmt/clippy/build/test 通过：719 项通过、0 失败、9 项按原条件未执行。通用 RuntimeStepInput、package descriptor、字符串命令与其他消费者的动态 ABI 尚未删除，未将完整 typed 迁移或性能验收标为完成。

- NativeVN Player 直接传递 NativeVnStepInput/NativeVnStepCommand，删除 VnPlayerCommand 到 action/argument/auxiliary/flag 的往返字符串转换及四类枚举映射；默认启动由 session 解析，旧 ABI 边界暂时转换到同一实现。显式 Launch 原先映射为 provider 不接受的字符串，现已修复。3 项针对性回归通过，覆盖指定剧情启动、布尔值/枚举、tick 失败和恢复；Engine 全量 fmt/clippy/build/test 通过：720 项通过、0 失败、9 项按原条件未执行。输出、package descriptor 和剩余 ABI 消费者尚未迁移。

- NativeVN step 输出改为直接拥有 PresentationCommand、VnAudioCommand 和 VnTimelineTask，Player 移动消费，删除内部 ABI 演出/timeline/音频转换及相应克隆。旧 ABI 只在返回边界适配；状态视图仍保留原有按需历史投影。新增音频缺失/多余/错序校验和超预算终止回归，provider 与 Player 单元测试通过。Engine 全量 fmt/clippy/build/test 通过：722 项通过、0 失败、9 项按原条件未执行；状态视图、package descriptor 和其他 ABI 消费者仍待迁移。

- NativeVnStateView 直接携带 typed 显示投影，Player 删除 RuntimeLiveVnState 和 cursor/choice/wait/system 等往返转换；ABI 仅在旧输出边界适配。4096 条 backlog 回归验证常规显示只取末条，Backlog/VoiceReplay/RouteChart 按页展开，终局保留路线，投影排除私有变量、调用栈和 read_state；2 项针对性测试通过。Engine 全量 fmt/clippy/build/test 通过：724 项通过、0 失败、9 项按原条件未执行；通用 package/lifecycle ABI 与其他消费者仍待迁移，真实平台验收未完成。

- NativeVN 新增 open_native 与 NativeVnSessionConfig，创建逻辑拆到 native_open；Player 与 runtime 共享同一 Arc<CompiledStory>，删除启动时的 Postcard 编码/hash/解码、临时 section、重复 prepare/probe 和通用 open request。原生入口支持无 package 嵌入；2 项回归验证剧情共享、无包存读档、恢复后推进、关闭取消、非法 worker 数与重复 session 不发布/覆盖。Engine 全量 fmt/clippy/build/test 通过：726 项通过、0 失败、9 项按原条件未执行；provider 内部 session map、save/lifecycle 数据类型与 package descriptor 仍待迁移。

- Player 的 NativeVnRuntimeHost 直接持有 NativeVnSession，step/save/restore 不再经 provider map；旧 ABI map 复用同一个 session 执行与关闭实现。创建、执行和保存模块已拆分，session API 校验调用身份，close 消费会话并取消作用域。provider 与 Player 单元测试通过，新增外来身份不变更状态、两个会话关闭隔离及 drop 取消回归。Engine 全量 fmt/clippy/build/test 通过：728 项通过、0 失败、9 项按原条件未执行；save/lifecycle ABI 数据类型、package descriptor 和剩余 ABI 消费者仍待迁移。

- 原生 session 与 Player 保存恢复改用 Runtime SaveBlob/LoadReport，删除实时原生路径的通用 save/restore request 和 section 包装。Player 存档升级 v8，旧 v7 实际字段布局、错误版本、容器损坏与外来 session 均拒绝；成功恢复取消旧作用域。provider/Player 单元测试通过；seed 校验前移至提交前，加入合法容器下 seed 不匹配回归。Engine 全量 fmt/clippy/build/test 通过：729 项通过、0 失败、9 项按原条件未执行；关闭 report、step identity 和 package descriptor 仍待清理。

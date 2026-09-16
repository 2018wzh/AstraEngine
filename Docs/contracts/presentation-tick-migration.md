# 演出帧推进迁移

本次变更落实[重构契约](rebuild.md)中的帧内状态所有权规则，适用于 `astra-vn-presentation` 的 `ProductStageDirector` 和 `PresentationCoordinator`。不新增通用任务引擎，不改变宿主权限或允许演出直接修改剧情 cursor。

## 执行和错误边界

`tick` 在原有 session 中推进 tween、timeline、文字、背景和视频时间，不克隆整个 director/coordinator，也不克隆每条活动 timeline 的完整 keyframe 列表。活动 timeline 暂时移出集合，由同一个 owner 采样并归还未完成的任务。

零 delta、超过一秒的 delta，以及 stage frame/time counter 溢出，在推进前拒绝；对象仍可使用。命令批次继续在提交边界准备 next state，失败不提交任何部分。排队 Show 的 layer/opacity、Movie 的 layer/alpha 和 timeline 的目标、属性、keyframe 范围也在进入队列前验证。`configure_text` 只有提交成功才消费 region sequence。

已经开始执行的 tick 若遇到目标消失、队列激活容量耗尽或内部状态不一致，返回原始 blocking diagnostic，并将该演出 session 标记为 failed。已经推进的帧不回滚。`is_failed()` 对外可查询；后续 tick、命令和 snapshot 返回 `ASTRA_VN_STAGE_SESSION_FAILED` 或 `ASTRA_VN_PRESENTATION_SESSION_FAILED`。无 Result 的文字请求和 activation drain 不再产生新操作。调用宿主须结束受影响的 session，不得忽略错误继续播放。其他 session 不受影响；恢复只能使用此前成功保存的 snapshot 或创建新 session，不能保存失败帧覆盖既有存档。

`prepare_batch` 和 resize 仍是显式边界事务。这里保留的 staging 不能重新移入每帧 tick。普通输入拒绝与执行失败分别有测试，不再用“每次 tick 全部回滚”作为错误恢复承诺。

## 并行轨道和完成

Timeline 的 `ReplaceTarget` 按 target/property 替换冲突轨道；同一目标的其他属性及其他 timeline 继续。camera 的 `main`、`camera`、`camera.main` 别名归一化后判断冲突。取消只移除指定 timeline，不伪造其完成通知。自然结束仍只返回一次 completion fence。

Region 队列保持原顺序并在活动过渡完成后激活；排队状态可存读档。背景增加 `transition_pending`，完成时只提交一次 incoming（包括显式清空），后续 tick 不擦除已显示背景。手动完成文字 reveal 后，下一帧不会把可见字数退回计时器计算值。

## 数据格式迁移

当前 Stage snapshot schema 为 `astra.vn.product_stage_state.v8`，coordinator 为 `astra.vn.presentation_coordinator.v5`。此前 v8/v4 布局已增加内部 failed 字段和背景 pending 标记；本轮 v5 增加等待组一致性校验，字段布局不变。`PresentationRegionCommand` 改用 serde 外部标记枚举，使非空队列可由 postcard 双向编码；旧内部标记表示只能写入，读取会失败。若使用 JSON，payload 形式相应改为如 `{"character": {...}}`，不再是 `{"region": "character", "command": {...}}`。

旧内部 snapshot 明确拒绝并重建，不提供迁移器。恢复时校验 coordinator schema、队列边界、区域匹配、排队策略和文字 reveal rate，拒绝损坏队列而不是延迟到 tick panic。商业游戏原生存档不属于此格式，不能覆盖。

## 验证边界

[Stage 回归](../../Engine/Source/Modules/AstraVN/astra-vn-presentation/tests/support/stage_tick.rs)覆盖 queued Move 的存读档、畸形排队命令拒绝、执行失败终止、从先前存档恢复、同目标不同属性继续、替换和取消。[Coordinator 回归](../../Engine/Source/Modules/AstraVN/astra-vn-presentation/tests/support/coordinator_tick.rs)覆盖多个 region 同时推进、一次性激活和 fence、显式清空及 reveal 不倒退。内部测试补充 counter 溢出和有界 activation 队列耗尽。

本项只证明普通 Rust 演出状态推进和保存边界。实际 Player 视听、性能预算和四平台播放仍须产品运行验证，不由本项单元测试关闭。

## 演出等待组

共用 fence id 的 Character/Background/Text/Video 命令组成 all-of 等待组，包含区域队列中的成员；同组未完成成员的 command id 必须唯一，冲突在批次提交前拒绝。只有所有成员都完成，coordinator 才发出一次完成通知；文字立即显示或视频先结束不能提前放行其他成员。任一成员失败或被新命令替换，组保持 Failed，后续成员完成不能覆盖失败；其他轨道继续执行。已经终结且没有活动成员的 fence id 可以用于新一组命令，重新进入 Pending。

成员身份直接来自现有活动/排队命令，保存同一 coordinator state，不另建线程池或任务 registry。新 coordinator schema 为 v5，旧 v4 快照拒绝重建；StageDirector 外层仍为 v8，恢复时校验内层 schema 与 fence 引用。跨区域并行、顺序排队、文字点击、视频完成/失败、替换与中途保存恢复均需要普通产品状态测试；通用 Runtime 任务组合和产品异步 IO 接入仍未完成。

## Player 呈现会话

NativeVN Player 的 frame tick 直接推进其 StageDirector，并按当前场景的资源需求生成输出；不克隆整个 director、stage state 或上一帧 scene 来回滚。delta 输入预检失败可重试；推进后发生资源、场景或渲染错误时，Player 呈现会话终止，后续 tick、剧情输入、渲染和保存拒绝。资源释放与 shutdown 仍可执行。

Player 检查当前 VN wait 对应的演出 fence；Failed 必须返回 `ASTRA_PLAYER_PRESENTATION_FENCE_FAILED`，不能静默等待或伪造完成。未被剧情等待的失败组不终止其他轨道。恢复在验证旧存档后重建呈现状态；提交前拒绝保留原会话状态，提交后的恢复失败保持终止，完整恢复成功才重新允许运行。此变更不改变存档字段布局。

读档首帧必须按恢复后的 stage 重建场景与纹理生命周期，不沿用读档前的 scene draw。活动转场逐帧借用已有 source snapshot，只有创建新转场或存读档边界才复制状态。

[Player 失败恢复回归](../../Engine/Source/Programs/astra-player-vn/src/native_vn_host/presentation_tests.rs)使用公开 package、字体与图片 fixture，覆盖资源失败后的帧状态、失败会话的输入/保存拒绝、恢复前后失败与成功重开，以及当前 fence 与无关失败组的隔离。

## Player 媒体结果作用域

`TaskScope::new` 允许不创建 World 的宿主持有根作用域；所有者负责在关闭时 cancel，子作用域与身份不进入存档。Player 发出的视频请求携带私有作用域，只能由当前 Player 创建。读档提交、请求替换和关闭使旧请求失效；视频 decode、帧提交和 fence 完成必须先验证作用域，不能仅凭相同 layer 名完成新请求。外部调用方使用 Player 发出的请求，不再自行构造 struct；存档只保存媒体数据，恢复时创建新作用域。

读档验证被拒绝时保留原请求和队列。Runtime 恢复一旦提交，Player 清空旧 timeline、音频、视频、stage completion 及待处理 UI/保存请求；后续呈现恢复失败也不能重新使用旧工作。资源缓存可保留，媒体 Host 必须保留旧视频流的关闭队列，在重新打开恢复的流之前关闭旧流，不能直接 clear 后遗失 native decode session。

Media Host 遇到已取消的视频只排队关闭，不能提交旧完成通知或把一次物理继续输入标为已消费。关闭失败保留当前及后续关闭任务，并返回错误。

## 产品存读档入口

原生 Player 与 Headless 共用 `prepare_product_save_transaction` / `restore_product_session`，把同一会话的音频、视频和 timeline snapshot 纳入存档。低层 `save` / `restore` 仍可用于没有 Media Host 的源状态测试，不能作为完整产品保存入口。产品读取缺媒体状态、错误媒体 schema 或不合法 timeline 时，在剧情恢复前拒绝；音频设备等执行阶段的恢复错误必须终止呈现会话，不能继续运行一半已恢复的产品。产品恢复成功后才向平台提交首帧，随后媒体处理关闭旧视频流并重建保存的流。

## 媒体播放时钟

Media Host 使用独立播放时间驱动 timeline deadline 和视频 `started_at_ms`，宿主 `now_ms` 只提供增量。首次处理绑定宿主时钟，播放时间从当前保存值继续；读档后清除宿主时钟绑定，首次处理不计入离开存档后的时间。后续宿主回退或播放时间溢出明确拒绝且不推进时钟。音频仍恢复其采样游标，不使用毫秒时钟改写 PCM 进度。

媒体 snapshot 升为 `astra.player.native_vn_media_snapshot.v3`，必需 `playback_time_ms`；旧 v2 拒绝重建。保存的 timeline 时刻与视频起点不得晚于播放时间。恢复后尚未重新打开的视频仍须进入再次保存的 snapshot，不能因未处理下一帧而丢失。

## 视频帧提交

视频帧绑定必须同时更新 CPU 缓存与场景的 `VideoFrame` 命令，命令携带当前像素、尺寸和目标区域，由 renderer 管理视频资源。不能以空呈现批次或仅更新 CPU 缓存代表视频播放。场景重建借用当前 StageDirector，不为每个视频帧复制整个演出状态。

恢复视频时按已保存的 cursor 解码并校验计数，只保留最后一个已呈现帧，再将该帧重新提交到场景；不能跳过它而沿用读档前纹理。恢复期间的解码、计数或呈现错误必须保留该流的关闭任务。

## 音频恢复预检

`AudioServiceSession::validate_timeline_restore` 在停止当前声音前检查设备格式、voice/bus 容量、序列、准备的 PCM、采样游标和渐变状态；失败保留当前 timeline 和声音。Player 的媒体预检在已打开音频服务时使用同一检查，不能等到剧情提交后才发现缺 PCM。此检查不代替冷启动时从 package 解码并准备保存的声音资产；没有资源时仍返回明确错误。

## 冷启动音频准备

产品 `restore_product_session` 改为 async，并接收当前平台 executor。恢复前从当前 package 重新读取缺失声音，走显式绑定 decoder 和与正常播放相同的 canonical PCM 准备路径；包身份、revision、资源 URI 或 canonical PCM 长度不匹配均失败。已准备的 PCM 可复用，未准备的资源按有界解码与缓存预算处理，不能重新播放剧情来填充缓存。准备和预检完成后才提交剧情与媒体恢复；低层同步 Media Host restore 仍只接受已准备资源。

### 读档输出端点边界

产品读档在预检和剧情提交后，重建已打开的音频输出端点与 Kira manager，复用已准备的共享 PCM 和保存的声音状态。旧 mixer 先停止并等待退出，再关闭旧 output，关闭成功后才打开同一显式 provider 的新 output；返回成功时不再消费旧端点队列。关闭或重建失败终止产品会话，保留可关闭 handle 供退出清理，不选择替代 provider。同步媒体 `restore` 只恢复内存状态，产品入口负责异步端点边界；真实设备已提交到硬件的采样与听感连续性另行实测。

### 视频启动失败的清理所有权

视频 decode open 成功后，Media Host 立即把逻辑 session 放入待关闭队列。描述校验和启动解码全部成功后才交给活动视频；启动失败立即尝试关闭，关闭失败保留队列供后续退出重试。等待启动 decode 的 future 被 drop 时同样保留 session，不能把已打开资源只留在局部变量中。取消请求在 open 后和 decode 返回后检查，旧请求不能接收新流。平台 open 命令自身被中断时的资源所有权仍须由平台 executor 单独处理。

### Player decoder open 的取消边界

PlatformCommandSink 持有尚未交付的 decoder open future，限制为 64 个；取消外层命令不会丢弃响应 receiver。正常返回后转入已打开 decoder 表，重复逻辑 id 在发送 open 前拒绝。媒体 shutdown 调用 cleanup_pending_decode_opens，等待被取消 open 的响应并关闭返回的 native session。清理自身被中断时保留 open/close future；关闭失败保留可重试状态，has_live_resources 包含这些未完成资源。清理不创建后台任务，sink 所有者必须等待清理结束后再销毁宿主。此约束覆盖 Player decoder 路径，直接 PlatformHostClient 调用和其他资源种类的取消仍单独推进。

### 音频 output 响应所有权

NativeVnProductAudioHost 持有唯一 pending open 和 pending close future。取消 ensure_open 后再次调用会继续同一 open；shutdown 则等待结果，直接关闭端点，不创建 Kira worker。关闭响应在恢复或退出 future 中断后继续由 Host 持有，下一次清理消费同一响应，不重复发送已完成的 close。明确失败的关闭仍可重试，端点在成功响应前保持所有权。这些 future 不进入存档，Host 必须在其平台客户端存活期间完成 shutdown。

### NativeVN 直接 Rust 宿主

Player 的 NativeVN 主路径直接持有 NativeVnRuntimeProvider，移除 ProductRuntimeHost 的同步/异步桥、session mailbox、通用 worker 调度与外层 mutex。私有 NativeVnRuntimeHost 只负责已选择 package binding、单 session 生命周期、tick/seed/mode、输出数量和 save section 边界；实际玩法仍由同一 NativeVN RuntimeWorld 执行。step/save 执行失败后拒绝继续，经过完整验证的 restore 可重建会话；输入 section 验证失败不提交恢复。关闭与销毁不再伪造插件 instance lifecycle。

现有 package/save 和 RuntimeStepInput 数据契约暂不改版；旧通用宿主仍供尚未迁移的非 Player 消费者使用，但 Player 不保留可切换的兼容分支。后续继续移除字符串 command、通用 product descriptor 和内部动态 ABI。这一步不把 NativeVN 全部 typed 重构标为完成。

### NativeVN typed 命令入口

NativeVnStepInput 将 tick/seed/mode 与 NativeVnStepCommand 分开表达。Player 直接传递已有 VnPlayerCommand；LaunchDefault 是独立变体，由权威 session 查找默认入口。删除 Player 的 runtime_step_fields 和 page/skip/reading/unlock 字符串映射，不再构造 RuntimeStepInput 的 action/argument/auxiliary/flag。NativeVnRuntimeProvider::step_native 是生产入口，旧 step 只在尚未迁移的 ABI 消费者边界解析字符串后调用同一实现。类型均为进程内数据，不新增序列化或存档字段；输出和 package descriptor 的后续迁移仍按总体计划执行。

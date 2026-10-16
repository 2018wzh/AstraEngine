# Independent Family API v13

`astra-emu-family-api` 是 Family 插件唯一必需的契约依赖。不依赖 RuntimeWorld、package/save、VFS、renderer/audio backend 或 UI。静态核心和动态核心共用 typed DTO；动态入口使用 `abi_stable` 的 `FamilyModule`，提供 descriptor/probe/open/advance/frame/close。

v13 在 v12 CPU lease/固定 owner 契约上增加 session-scoped typed 模态服务，消费者同批迁移；Manager 宿主服务与 Slint 原按钮/输入/取消已实际验证，KrKr native dialog/NativeOwner 接入和真实游戏验收仍未完成。统一重建插件，不保留旧同步帧 visitor、旧 vtable 或模态默认答案。

## 启动配置

`FamilyDescriptor.configuration` 是有序 `ConfigField` 列表；字段包含唯一 ASCII ID、UTF-8 label、可为空的显示 group、`ConfigKind` 和必需 default。group 为平面显示分组，不改变 key，也不支持递归。`OpenRequest.configuration` 为 `ConfigEntry` 列表，值类型为 Bool、Integer(i64)、Number(f64)、String 或 Enum。String 与 Enum 不互换，不做字符串到数字的隐式转换。

最多 128 个字段/值，ID 最多 128 bytes，只接受字母、数字和 `._-`；label/group 最多 256 bytes，不允许控制字符；String 上限最多 4096 UTF-8 bytes，不允许 NUL；Enum 最多 128 个唯一选项，选项遵循 ID 规则。Integer/Number 范围含端点，Number 的范围和数值都必须有限。默认值必须符合声明的类型和范围。

`validate_config_schema` 校验完整 schema；`resolve_config` 拒绝未知/重复 key、类型不匹配、无效默认值和越界，再按 schema 顺序补齐缺省值。错误 `ASTRA_EMU_FAMILY_CONFIG` 包含字段/值 index 和原因，不包含用户值。Host 在调用核心前验证，核心仍须调用 `validate_for_descriptor` 和 `resolve_config`。配置只在启动时生效，不能在线修改运行中的核心。配置不授予文件/网络等额外 Host 权限。

Manager SQLite schema 4 按 plugin ID + game ID 保存 typed 值；空 game ID 为核心默认值，游戏覆盖优先，再使用 schema 默认值。每层先验证，未知旧键不会在合并时丢失。动态 Family 二进制不写入 SQLite：Manager 启动时只扫描数据目录下的 `cores/`，按平台动态库扩展和 `astra_emu_`（Unix 也接受 `libastra_emu_`）文件名前缀发现核心，再复用 ABI、descriptor 和 capability 校验。现有旧 schema 中的 `plugin_installation` 表不再读取或写入，也不是插件注册来源；它可以随旧数据库保留，不触碰资料库、配置或游戏原生存档。更新核心需要替换 `cores/` 文件并重启 Manager，运行期间不热加载。操作见 [配置手册](CONFIGURATION.md)。

## 服务、输入和资源

Descriptor 必须声明 CpuFrame，拒绝重复 capability/format。`PcmAudio` 声明仍要求 Host audio sink，open 返回格式必须与 capability 一致。Family 在任何 write 前用返回的固定格式调用 configure。PCM 为有界可取消队列，设备回调不能调用 Family；格式、channel alignment、chunk 上限和有限 F32 值均验证。

`TextReplacement` 仅表示可选能力：不传 service 为本次关闭翻译，不阻止基础播放。服务使用 typed reset/submit/poll/cancel；poll 为 Pending、Ready、Cancelled 或 Failed。正文缓存不超过最近八段、6000 字符，日志不含正文。FVP 不声明此能力。

### 模态宿主服务（v13 实施契约）

`FamilyHostServices.modal` 为可选 `ModalServiceBox`，每次 open 创建独立服务。未提供服务不妨碍无模态请求的基础播放；核心真正请求时必须明确返回 unavailable，不能自动确认、伪造输入或用既有默认按钮代替。服务只传 UTF-8 `ModalRequest`/`ModalResponse`，不传 UI 类型、native handle 或原生 callback。

请求保留 caption/message 和原按钮顺序/标签；`initial_text=None` 是消息框，Some（含空串）是输入框。caption/message/输入各最多 65,536 UTF-8 bytes，最多 16 个按钮、每个最多 256 bytes，拒绝 NUL，不截断。响应为 Cancelled 或 Submitted，button index 必须属于原请求，输入值的有无与请求一致。窗口关闭和 session 取消都不能当作按钮 0/空输入确认，错误不含正文。

`request` 只能在 core worker 等待，窗口线程继续呈现、接收真实用户响应或取消；Host 同一 session 最多保留一个待答请求，满时明确失败。关闭先 one-shot `cancel` 唤醒等待，再 join worker；旧 service 及迟到 UI response 不能跨 epoch/绑定新 session。读档/重开失效请求必须走同样取消边界。无窗口 Host 显式拒绝请求，不新增常驻轮询、每帧序列化或 UI/provider shim。


### 音频播放进度（v10 实施契约）

v10 在 `AudioSink` 增加一个可选查询 `playback_progress() -> FfiFamilyResult<AudioPlaybackProgress>`。`AudioPlaybackProgress` 只含 `accepted_source_frames: u64`、`played_source_frames: u64` 和 `AudioPlaybackClock`，后者为 `DeviceTimestamp` 或 `TestConsumption`。vtable 与指纹统一升级为 v10，旧插件重建，不保留 ABI 兼容层。原有 sink 的默认实现明确返回 unsupported，未使用查询的核心无需增加实现。

一帧包含 configure 声明的全部声道，计数采用该源采样率，与设备声道数和采样率无关。accepted 只累计 Host 实际接纳的 PCM，包括重采样器尚未输出的输入；写入失败或取消时，未被 Host 接纳的块或部分不能增加计数。played 是最近一次设备时间戳采样对应的源帧播放水位，必须满足 `played <= accepted`。两者之差覆盖重采样器输入留存及滤波延迟、Host 队列、当前块未消费部分和已经提交设备但尚未到播放时点的部分。设备欠载填充的静音不属于 Family PCM，不推进源帧计数。换算使用整数有理数，保留采样率转换余数，不能逐块截断后累加。

`DeviceTimestamp` 使用实际设备回调提供的 callback/playback 时间。查询只能读取最近采样，不用查询时的墙钟外推，也不把回调提交位置当成播放位置；它受后端时间戳精度和回调周期限制，不承诺绝对 DAC 时刻。首个设备回调前 played 为零。支持性按实际后端确定，缺少有效时间戳的后端返回 `ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED`。现有无需查询的 Family 继续基础播放；确实依赖进度的 Family 在 configure 后预检，不能用零或估算值替代不支持错误。

`TestConsumption` 只供显式 NullAudio，按测试设备实际消费的有效帧推进，保留测试输出标记。它不代表可听声音。configure 每个 sink 只能成功一次；重开 session 创建新 sink 并从零计数，旧 sink 不能绑定新设备。未 configure、取消、关闭、设备失败和计数溢出分别返回明确错误；取消或丢弃数据不算已播放，旧 sink 的晚到查询不能读取新 session。查询允许与 worker 写入、设备消费和关闭并发，返回一致的两计数快照；设备回调不调用 Family，最终错误由 session owner 统一处理。

增量测试覆盖首次回调、部分块、声道和采样率转换、重采样启动延迟与尾部留存、欠载静音、取消阻塞写入、重复 configure、旧 sink 晚查，以及 unsupported 与设备失败。真实设备播放仍须单独运行，不以这些单元测试宣称完成。

Manager 同采样率/声道的 F32 chunk 保留原 ABI `RVec` owner 到设备消费，不转换成宿主 Vec，不做 planar/interleaved 重排；不同格式、声道或采样率仍使用实际转换。设备回调只向最终输出缓冲写样本，完成块移入 lock-free retirement ring；producer 在下次写入前回收，关闭在 callback context 销毁确认后回收剩余块。ring 容量为 queue + current + 一个串行阻塞 producer（当前为 4+2），不在回调释放 PCM 分配。关闭持有 ready receiver，避免驱动线程成为队列最后 owner；异常关闭仍继续清理并汇总错误。

`AudioSink::cancel` 是整个 session 的 one-shot 取消，断开一个通知域唤醒全部排队/串行化 writer，不为每块创建等待通道。Siglus 保留两个块的桥接队列和每个原生 mixer 的独立取消域，旧 mixer 析构不能调用 session cancel；已经进入 Host write 的块按 Host FIFO 接纳/取消，未接纳或取消块不增加 accepted，丢弃不冒充 played。借用的原生 F32 tap 跨 worker 必须复制一次以取得独立 lifetime，保留 F32、不做 I16 量化；无需修改上游 mixer 或音频 API。

Host 只提供 game_path、初始 WindowState 和输入。Family 自持 VM、文件、解码、混音、渲染和原生存档。probe 返回 `ROption<ProbeReport>`，None 是普通未匹配；game_id 为 opaque UTF-8。事件保留键盘、指针、滚轮、文本及窗口顺序。PointerMove 为 Host 逆映射后的逻辑舞台坐标，resize 为物理客户区像素；elapsed_ns 接受包括零在内的 u64，由核心决定 tick。

`OpenRequest` 携带 Host 分配的非零 `session_epoch`。CPU 帧改由 `FramePoll::Ready(FrameLease) / Unchanged(FrameIdentity) / Pending(FrameIdentity)` 交付；`FrameIdentity` 包含 epoch 与 generation。首个完成帧 generation 非零，之后只在新完成输出、resize 或恢复失效时递增，不按 Host 拉取次数递增，不逐帧 hash 推断变化。读档/重新开始可重置原生时钟，但同一 session 内不能倒退帧 generation。`Pending` 明确表示仍有输出待完成，不能冒充无变化。

`FrameLease` 持有 `abi_stable` 共享、不可变 CPU 像素 owner，Host 保留到 GPU 上传完成；clone 只增加 owner 引用，不复制像素。Family 包装已有 Vec、Arc 或共享字节 owner，不强迫上游改成 Astra 容器。owner 不能包含 VM、GPU、窗口或需要会话线程析构的原生资源；close 停止生产并 join worker，但已交付的 CPU lease 仍可安全读取/释放。生产缓冲只能在没有消费者引用时复用，缓冲耗尽须背压或返回 Pending，不能覆盖消费者正在读取的内存或无限扩容。

`FrameInfo.width/height` 是物理 raster，`logical_width/logical_height` 是 VM/输入舞台；stride、尺寸、长度和像素语义明确验证。格式声明 RGBA/BGRA 顺序、sRGB/linear 编码，行声明 top-to-bottom/bottom-to-top；alpha 声明 opaque 或 ignored。ignored 表示 RGB 已按原生规则最终合成，alpha 字节不参与 Host 透明度，不需要 Family 扫描改写 alpha。Host 直接按原生 stride 上传，不为 `Queue::write_texture` 增加 256-byte 行填充；必要的翻转、颜色/alpha 呈现转换由 Host GPU 完成。普通 RGBA/sRGB/top-to-bottom/opaque 无滤镜输出直接呈现上传纹理。不跨 ABI 传 GPU/native handle，不改变上游混合空间。

已迁移 Family 的最终合成 RGB 使用 Ignored alpha，避免发布时重新扫描/改写未被消费的 alpha；公共 Opaque 仍严格验证。Host GPU 归一化成本及实际验收范围见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md#已验证范围)。KrKr 本批跳过，不计整合验收。

推进、取帧与呈现解耦。每个 Family 的 provider、VM、GPU 与生命周期入口从创建到关闭固定在同一 owner；UI 只提交有界 typed 命令并接收结果/最新帧，不能直接执行核心推进或等待自己负责完成的模态请求。输入保持顺序，帧消费者可以降频但不能丢逻辑步、篡改原生音频时钟或存档语义。epoch 切换/关闭后旧输入、完成结果和帧拒绝，设备重建仍可重新上传保留 lease 的完整基线。

Manager 在固定 `astra-family-owner` 线程构造并加载 registry；`Rc` provider/session 不跨线程，也不要求 `Send`。UI 缓存描述符/schema，只提交容量 8 的 typed 命令；普通输入、窗口生命周期、关闭按 epoch 排序。开档与正常关闭异步，native 入口阻塞不阻塞 UI；关闭完成前禁止新开档，窗口退出状态不被旧 owner 属性恢复。逻辑 60 Hz 期限、PCM health、advance 与 acquire_frame 都在 owner，UI 的服务计时器仅排队输入/应用元数据，不重新锚定播放时钟。窗口结果有界合并，标题只在改变时复制；最新 CPU lease 直接进入 mailbox。最终应用退出 join owner。Manager 模态服务遵循 v13 的真实响应与取消边界；具体核心接入和产品验收见审计，KrKr 本批按用户决定跳过，无窗口 Host 明确拒绝请求。

Manager 的原生会话 owner 一次性保留 8 MiB 线程栈，供未经重写的核心初始化临时值使用，不逐帧分配。Headless 同样在独立固定 owner 上完成加载、open、advance、取帧和 close，不能回到 Windows 窗口主线程的小默认栈；不在线程之间搬移已创建的 VM/GPU。这是宿主资源预算，不新增或改变 Family ABI。

## 窗口请求

`AdvanceResponse.reset_clock` 默认 false。成功读档、重新开始或 Family 定义的暂停/恢复生命周期建立新的播放时间线时，核心提交一次 true；实时 Host 在本次 advance 和最终帧复制完成后重新计算计时起点与下一帧期限，避免把恢复、暂停或生命周期处理耗时计入新时间线。窗口暂停期间的墙钟累计由 Family 丢弃，恢复调用可以消费当前正常帧间隔，但不能补跑暂停累计。普通活动帧仍计入核心执行耗时，`elapsed_ns > 1s` 的 Musica 活动帧仍返回有界诊断，不静默截断。失败操作不提交重置；固定步长 Headless 不依赖墙钟，无需调整其步长。

`AdvanceResponse.window_command` 为可选的 `FamilyWindowCommand`，包含 `SetFullscreen`、`SetTitle`、`SetIcon` 和 `SetPointer`。每次 advance 最多提交一个窗口请求，无 native handle、回调或 UI 类型穿过 ABI。Manager 在窗口线程应用请求；设置保留到下一个请求或会话关闭，退出及错误关闭恢复普通窗口和可见光标。无窗口的 Headless Host 遇到请求返回 `ASTRA_EMU_HEADLESS_WINDOW_COMMAND_UNAVAILABLE` 并走正常关闭流程，不静默忽略。核心在原生设置应用成功时提交请求，取消草稿不提交；需要恢复全屏的冷启动在首次 advance 提交。

v11 增加 `SetPointer(FamilyPointerRequest)`。`logical_x`、`logical_y` 为可空的 i32 舞台坐标，`visible` 为可空 bool，`autohide_ms` 为可空 u64 毫秒数；空值保留当前属性，零时长关闭自动隐藏。Host 按实际游戏 content viewport 和 DPI 将舞台位置正向映射到窗口位置，只更改指定轴。自动隐藏使用 Host 的单次计时器；实际指针移动或按钮操作重置计时，显式隐藏仍保持隐藏。关闭、失败及 Manager 覆盖层恢复应用光标；回到游戏恢复游戏设置。后端不能移动系统光标或定时器时长无法表示时明确失败，不伪造成功。Family 合并尚未交付的同属性更新，但必须保留不同类型的请求，不能让 mouse 覆盖 caption。所有动态插件随 v11 重建，不保留 v10 兼容层。

v8 增加 `FamilyWindowCommand::SetTitle(RString)`：标题为最多 4096 UTF-8 bytes 的文本，空串明确清空标题，NUL 和 Unicode 控制字符拒绝，不截断。旧核心字符串由 Family 严格按原生编码解码，Host 不猜测编码。`validate` 失败返回 `ASTRA_EMU_FAMILY_WINDOW_TITLE`，错误和日志不含标题正文。每次 advance 仍最多一个窗口请求，Manager 在窗口线程将最新请求同步到实际窗口标题；关闭/失败恢复 Manager 标题。NullAudio 测试标记不受游戏标题覆盖。无窗口 Host 与全屏请求一样明确拒绝，不能将标题命令当作成功的空操作。

## Manager 诊断

加载器必须先调用 `initialize_diagnostics(DiagnosticSinkBox)`，成功后才能读取 descriptor、probe 或 open。sink 支持并发调用，作用域为进程，不捕获游戏、窗口、音频或 session 资源，也不得回调 Family。库驻留期间保留首个 sink；重复加载不叠加 subscriber。静态核心不安装桥，直接使用宿主 tracing/log 订阅器。

API 的可选 `diagnostic-bridge` feature 复用 tracing-subscriber 与 tracing-log，供 FVP、Siglus、Musica 动态适配层使用；纯 ABI 消费者无需这些依赖。安装冲突返回 `ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT` 并阻止加载。桥不打开文件、不另建队列或日志线程；Manager 通过既有 astra-observability 管理输出、过滤、丢弃统计和 flush。

`DiagnosticEvent` 最多 32 个唯一字段，字段名使用最多 128 bytes 的 ASCII 标识符；target/event 和 Text 最多 4096 bytes，数值必须有限。日志桥不做内容脱敏或白名单过滤，字符串、message 和 Debug/Display 正常转发。超限、重复字段和非法数值累计 `dropped_fields`；格式化 Debug 时使用有界 writer。未结构化日志标为 `family.unstructured_log`，保留来源和源码行号。

Manager 再验证边界和重复字段；非法记录输出 `family.diagnostics.invalid`，不打印原记录。正常记录在 `astra_emu::family` target 输出，原来源为 `core_target`。关键诊断仍需稳定事件和安全字段，错误继续由原操作返回，不以日志代替错误处理。

## 生命周期与迁移

ABI fingerprint 为 `astra.emu.independent_family_abi.v13`，schema 为 `astra.emu.independent_family_api.v13`。v1 至 v12 插件必须重新构建安装，不提供兼容 reader/adapter。

动态库首次加载后一直驻留到进程退出，失败加载产生的 ABI 元数据也不会指向卸载的代码；更新插件必须重启 Manager。provider/module 对象按正常生命周期释放。会话 close 和 open 失败仍须取消 Host 请求、唤醒阻塞 PCM 写入并等待所有 worker 结束，错误不得跳过其他 worker 清理。驻留不允许保留已关闭会话的线程或 session callback；无 session 资源的进程级诊断 sink 除外。

验证使用普通 Rust unit/integration test。配置合法/默认/边界/损坏值、可选翻译和必需音频、Manager 持久化覆盖及失败写入保护均需通过；真实游戏输入、音视频、原生存读档和退出仍须实际运行，局部测试不代表产品验收。


v9 增加 `SetIcon(FamilyWindowIcon)`，只传 owned、紧密排列的 straight-alpha RGBA8 像素与宽高；每轴 1–256，字节数严格等于宽×高×4。尺寸、溢出或长度错误返回 `ASTRA_EMU_FAMILY_WINDOW_ICON`，不截断、不传文件路径或 native handle。此上限是 Host 边界，不宣称为旧引擎上限。Manager 复用窗口后端的 small-icon 能力，关闭或失败后恢复应用图标；无窗口或不支持该能力的后端显式拒绝。原生资源读取、解码与 best-effort 失败语义归 Family；未发请求不能称图标已显示。每次 advance 仍只提交一个命令，Family 必须保留不同类型的待提交请求，不能用图标覆盖未交付标题。

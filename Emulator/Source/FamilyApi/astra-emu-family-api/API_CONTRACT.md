# Independent Family API v10

`astra-emu-family-api` 是 Family 插件唯一必需的契约依赖。不依赖 RuntimeWorld、package/save、VFS、renderer/audio backend 或 UI。静态核心和动态核心共用 typed DTO；动态入口使用 `abi_stable` 的 `FamilyModule`，提供 descriptor/probe/open/advance/frame/close。

## 启动配置

`FamilyDescriptor.configuration` 是有序 `ConfigField` 列表；字段包含唯一 ASCII ID、UTF-8 label、可为空的显示 group、`ConfigKind` 和必需 default。group 为平面显示分组，不改变 key，也不支持递归。`OpenRequest.configuration` 为 `ConfigEntry` 列表，值类型为 Bool、Integer(i64)、Number(f64)、String 或 Enum。String 与 Enum 不互换，不做字符串到数字的隐式转换。

最多 128 个字段/值，ID 最多 128 bytes，只接受字母、数字和 `._-`；label/group 最多 256 bytes，不允许控制字符；String 上限最多 4096 UTF-8 bytes，不允许 NUL；Enum 最多 128 个唯一选项，选项遵循 ID 规则。Integer/Number 范围含端点，Number 的范围和数值都必须有限。默认值必须符合声明的类型和范围。

`validate_config_schema` 校验完整 schema；`resolve_config` 拒绝未知/重复 key、类型不匹配、无效默认值和越界，再按 schema 顺序补齐缺省值。错误 `ASTRA_EMU_FAMILY_CONFIG` 包含字段/值 index 和原因，不包含用户值。Host 在调用核心前验证，核心仍须调用 `validate_for_descriptor` 和 `resolve_config`。配置只在启动时生效，不能在线修改运行中的核心。配置不授予文件/网络等额外 Host 权限。

Manager SQLite schema 4 按 plugin ID + game ID 保存 typed 值；空 game ID 为核心默认值，游戏覆盖优先，再使用 schema 默认值。每层先验证，未知旧键不会在合并时丢失。动态 Family 二进制不写入 SQLite：Manager 启动时只扫描数据目录下的 `cores/`，按平台动态库扩展和 `astra_emu_`（Unix 也接受 `libastra_emu_`）文件名前缀发现核心，再复用 ABI、descriptor 和 capability 校验。现有旧 schema 中的 `plugin_installation` 表不再读取或写入，也不是插件注册来源；它可以随旧数据库保留，不触碰资料库、配置或游戏原生存档。更新核心需要替换 `cores/` 文件并重启 Manager，运行期间不热加载。操作见 [配置手册](CONFIGURATION.md)。

## 服务、输入和资源

Descriptor 必须声明 CpuFrame，拒绝重复 capability/format。`PcmAudio` 声明仍要求 Host audio sink，open 返回格式必须与 capability 一致。Family 在任何 write 前用返回的固定格式调用 configure。PCM 为有界可取消队列，设备回调不能调用 Family；格式、channel alignment、chunk 上限和有限 F32 值均验证。

`TextReplacement` 仅表示可选能力：不传 service 为本次关闭翻译，不阻止基础播放。服务使用 typed reset/submit/poll/cancel；poll 为 Pending、Ready、Cancelled 或 Failed。正文缓存不超过最近八段、6000 字符，日志不含正文。FVP 不声明此能力。

### 音频播放进度（v10 实施契约）

v10 在 `AudioSink` 增加一个可选查询 `playback_progress() -> FfiFamilyResult<AudioPlaybackProgress>`。`AudioPlaybackProgress` 只含 `accepted_source_frames: u64`、`played_source_frames: u64` 和 `AudioPlaybackClock`，后者为 `DeviceTimestamp` 或 `TestConsumption`。vtable 与指纹统一升级为 v10，旧插件重建，不保留 ABI 兼容层。原有 sink 的默认实现明确返回 unsupported，未使用查询的核心无需增加实现。

一帧包含 configure 声明的全部声道，计数采用该源采样率，与设备声道数和采样率无关。accepted 只累计 Host 实际接纳的 PCM，包括重采样器尚未输出的输入；写入失败或取消的整块不能增加计数。played 是最近一次设备时间戳采样对应的源帧播放水位，必须满足 `played <= accepted`。两者之差覆盖重采样器输入留存及滤波延迟、Host 队列、当前块未消费部分和已经提交设备但尚未到播放时点的部分。设备欠载填充的静音不属于 Family PCM，不推进源帧计数。换算使用整数有理数，保留采样率转换余数，不能逐块截断后累加。

`DeviceTimestamp` 使用实际设备回调提供的 callback/playback 时间。查询只能读取最近采样，不用查询时的墙钟外推，也不把回调提交位置当成播放位置；它受后端时间戳精度和回调周期限制，不承诺绝对 DAC 时刻。首个设备回调前 played 为零。支持性按实际后端确定，缺少有效时间戳的后端返回 `ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED`。现有无需查询的 Family 继续基础播放；确实依赖进度的 Family 在 configure 后预检，不能用零或估算值替代不支持错误。

`TestConsumption` 只供显式 NullAudio，按测试设备实际消费的有效帧推进，保留测试输出标记。它不代表可听声音。configure 每个 sink 只能成功一次；重开 session 创建新 sink 并从零计数，旧 sink 不能绑定新设备。未 configure、取消、关闭、设备失败和计数溢出分别返回明确错误；取消或丢弃数据不算已播放，旧 sink 的晚到查询不能读取新 session。查询允许与 worker 写入、设备消费和关闭并发，返回一致的两计数快照；设备回调不调用 Family，最终错误由 session owner 统一处理。

增量测试覆盖首次回调、部分块、声道和采样率转换、重采样启动延迟与尾部留存、欠载静音、取消阻塞写入、重复 configure、旧 sink 晚查，以及 unsupported 与设备失败。真实设备播放仍须单独运行，不以这些单元测试宣称完成。

Host 只提供 game_path、初始 WindowState 和输入。Family 自持 VM、文件、解码、混音、渲染和原生存档。probe 返回 `ROption<ProbeReport>`，None 是普通未匹配；game_id 为 opaque UTF-8。事件保留键盘、指针、滚轮、文本及窗口顺序。PointerMove 为 Host 逆映射后的逻辑舞台坐标，resize 为物理客户区像素；elapsed_ns 接受包括零在内的 u64，由核心决定 tick。

`FrameView<'a>` 借用 CPU RGBA8 sRGB opaque 像素；`FrameConsumerRef<'_>` 只可在同步 frame 调用内使用。`FrameInfo.width/height` 是物理 raster 尺寸，`logical_width/logical_height` 是场景和输入使用的逻辑舞台尺寸。两者可以有不同宽高比；Family 用统一比例渲染到居中的 content viewport，viewport 外的 raster 像素必须是黑边，Host 负责在窗口中继续按相同比例显示并逆映射输入。Host 返回前复制 stride × height 字节，不跨 ABI 传 GPU/native handle。

## 窗口请求

`AdvanceResponse.reset_clock` 默认 false。成功读档、重新开始或 Family 定义的暂停/恢复生命周期建立新的播放时间线时，核心提交一次 true；实时 Host 在本次 advance 和最终帧复制完成后重新计算计时起点与下一帧期限，避免把恢复、暂停或生命周期处理耗时计入新时间线。窗口暂停期间的墙钟累计由 Family 丢弃，恢复调用可以消费当前正常帧间隔，但不能补跑暂停累计。普通活动帧仍计入核心执行耗时，`elapsed_ns > 1s` 的 Musica 活动帧仍返回有界诊断，不静默截断。失败操作不提交重置；固定步长 Headless 不依赖墙钟，无需调整其步长。

`AdvanceResponse.window_command` 为可选的 `FamilyWindowCommand::SetFullscreen(bool)` 或 `SetTitle(RString)`。每次 advance 最多提交一个窗口请求，无 native handle、回调或 UI 类型穿过 ABI。Manager 在窗口线程应用请求；设置保留到下一个请求或会话关闭，退出及错误关闭恢复普通窗口。无窗口的 Headless Host 遇到请求返回 `ASTRA_EMU_HEADLESS_WINDOW_COMMAND_UNAVAILABLE` 并走正常关闭流程，不静默忽略。核心在原生设置应用成功时提交请求，取消草稿不提交；需要恢复全屏的冷启动在首次 advance 提交。

v8 增加 `FamilyWindowCommand::SetTitle(RString)`：标题为最多 4096 UTF-8 bytes 的文本，空串明确清空标题，NUL 和 Unicode 控制字符拒绝，不截断。旧核心字符串由 Family 严格按原生编码解码，Host 不猜测编码。`validate` 失败返回 `ASTRA_EMU_FAMILY_WINDOW_TITLE`，错误和日志不含标题正文。每次 advance 仍最多一个窗口请求，Manager 在窗口线程将最新请求同步到实际窗口标题；关闭/失败恢复 Manager 标题。NullAudio 测试标记不受游戏标题覆盖。无窗口 Host 与全屏请求一样明确拒绝，不能将标题命令当作成功的空操作。

## Manager 诊断

加载器必须先调用 `initialize_diagnostics(DiagnosticSinkBox)`，成功后才能读取 descriptor、probe 或 open。sink 支持并发调用，作用域为进程，不捕获游戏、窗口、音频或 session 资源，也不得回调 Family。库驻留期间保留首个 sink；重复加载不叠加 subscriber。静态核心不安装桥，直接使用宿主 tracing/log 订阅器。

API 的可选 `diagnostic-bridge` feature 复用 tracing-subscriber 与 tracing-log，供 FVP、Siglus、Musica 动态适配层使用；纯 ABI 消费者无需这些依赖。安装冲突返回 `ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT` 并阻止加载。桥不打开文件、不另建队列或日志线程；Manager 通过既有 astra-observability 管理输出、过滤、丢弃统计和 flush。

`DiagnosticEvent` 最多 32 个唯一字段，字段名使用最多 128 bytes 的 ASCII 标识符；target/event 和 Text 最多 4096 bytes，数值必须有限。日志桥不做内容脱敏或白名单过滤，字符串、message 和 Debug/Display 正常转发。超限、重复字段和非法数值累计 `dropped_fields`；格式化 Debug 时使用有界 writer。未结构化日志标为 `family.unstructured_log`，保留来源和源码行号。

Manager 再验证边界和重复字段；非法记录输出 `family.diagnostics.invalid`，不打印原记录。正常记录在 `astra_emu::family` target 输出，原来源为 `core_target`。关键诊断仍需稳定事件和安全字段，错误继续由原操作返回，不以日志代替错误处理。

## 生命周期与迁移

ABI fingerprint 为 `astra.emu.independent_family_abi.v10`，schema 为 `astra.emu.independent_family_api.v10`。v1/v2/v3/v4/v5/v6/v7/v8/v9 插件必须重新构建安装，不提供兼容 reader/adapter。

动态库首次加载后一直驻留到进程退出，失败加载产生的 ABI 元数据也不会指向卸载的代码；更新插件必须重启 Manager。provider/module 对象按正常生命周期释放。会话 close 和 open 失败仍须取消 Host 请求、唤醒阻塞 PCM 写入并等待所有 worker 结束，错误不得跳过其他 worker 清理。驻留不允许保留已关闭会话的线程或 session callback；无 session 资源的进程级诊断 sink 除外。

验证使用普通 Rust unit/integration test。配置合法/默认/边界/损坏值、可选翻译和必需音频、Manager 持久化覆盖及失败写入保护均需通过；真实游戏输入、音视频、原生存读档和退出仍须实际运行，局部测试不代表产品验收。


v9 增加 `SetIcon(FamilyWindowIcon)`，只传 owned、紧密排列的 straight-alpha RGBA8 像素与宽高；每轴 1–256，字节数严格等于宽×高×4。尺寸、溢出或长度错误返回 `ASTRA_EMU_FAMILY_WINDOW_ICON`，不截断、不传文件路径或 native handle。此上限是 Host 边界，不宣称为旧引擎上限。Manager 复用窗口后端的 small-icon 能力，关闭或失败后恢复应用图标；无窗口或不支持该能力的后端显式拒绝。原生资源读取、解码与 best-effort 失败语义归 Family；未发请求不能称图标已显示。每次 advance 仍只提交一个命令，Family 必须保留不同类型的待提交请求，不能用图标覆盖未交付标题。

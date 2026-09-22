# 全产品重构契约

本契约记录用户确认的新边界，实施状态见 [实施计划](../status/implementation-plan.md)。旧契约只在尚未迁移的实现中描述现状，冲突时以本契约为准。

## Engine 与共享库

公共 crate 使用 `astra-runtime`，公共会话使用 `EngineSession`；公共场景、任务、时钟、输入和呈现不得归入名为 AstraVNRuntime 的层。`VnSession` 持有 VN 剧情和系统状态，`VnRuntime` 名称仅用于 VN 业务。迁移必须调整实际所有权及调用方，不增加旧名称兼容别名。

共享文字、图像、绘制、解码、混音与字节源可不创建 World/package/registry 而使用。Engine/VN 与 Musica 是实际消费者。NativeVN 以普通 Rust typed API 组合，FSM 不作为唯一修改入口；固定 60 Hz 逻辑和独立呈现共用明确生命周期。任务具有句柄、作用域、完成/取消/失败结果；旧代次结果拒绝。存档保存显式状态，删除通用回放和帧内全量事务。

`astra-vn::VnSession` 是 NativeVN 的 typed 产品入口，持有剧情、变量和系统状态，并组合 `astra-runtime::EngineSession`。EngineSession 创建和持有 RuntimeWorld、任务作用域及逻辑步生命周期；VN 在修改剧情前校验步序、seed 和恢复模式。关闭会话取消其任务，不影响其他会话。保存仍使用原有 SaveBlob 容器，读取失败保留当前状态。

旧 `astra-vn-runtime-provider` crate、NativeVnRuntimeProvider/Factory、FFI 转换和会话 map 已删除。Player、CLI、发布检查改用 `astra-vn`；`native_vn_descriptor()` 只提供现有包格式需要的元数据，不创建或选择动态 provider。其他公共 gameplay/UI ABI 消费者仍须继续迁移，不因本次删除视为全部完成。

## EMU Family API

Manager headless 使用现有 `astra-observability` 的有界日志，与本次输出图像放在一起：将输出文件扩展名替换为 `.diagnostics` 作为日志目录。日志不写入游戏目录或存档，不属于 package；测试结束后按临时产物规则清理。

共享音频的流式解码只将解码器明确返回的流结束视为完成。`SymphoniaAudioStreamDecoder` 遇到意外 EOF 返回 `ASTRA_AUDIO_STREAM_TRUNCATED_INPUT`；已输出的有效 PCM 前缀不代表整段播放成功，调用方须终止失败的播放请求。正常完整流和解码预算规则不变。

GPU 回读缓冲可以在进程内保留共享所有权。`OwnedPixelBuffer::from(Arc<[u8]>)` 复用原像素分配，克隆只延长生命周期，`make_mut_for_update` 继续按写时复制隔离修改；转换本身不校验图像尺寸，仍由 `TextureFrame` 的现有校验入口负责。Musica 直接持有回读缓冲，CMVS 将同一缓冲转为纹理；Family 帧借用有效期和 Host 同步复制约束不变，保存与 ABI 格式不变。

公共 `astra-media-core` 的 `Extent2D`/`Canvas2D` 负责逻辑舞台到 raster canvas 的比例校验、根变换和坐标映射，并复用 `RectI`、`Transform2D`、`TextureFrame`、`SceneCommand`。四个空间必须分开：逻辑舞台决定 VM、存档、时间和动画几何；资产保存物理像素并显式声明逻辑尺寸及格式原点；raster canvas 是 GPU 目标；窗口输出由 Host 做 letterbox 和 pointer 逆映射。`astra-emu-sdk` 只提供薄语义 `StageCanvas` 与 `TextureAsset`，不依赖 VN；AstraVN presentation 直接消费同一 `Canvas2D`，作为第二个真实调用方。

Musica 固定逻辑舞台 `1280x720`，启动配置显式提供正整数 `render_width` 与 `render_height`，按设备和内存能力选择任意 raster 尺寸。配置无效或 raster 预算不足必须失败，不能静默降级。不同宽高比使用统一 scale 的居中 aspect-fit viewport，剩余像素清黑；viewport 外的指针不进入游戏。物理像素密度不能改变逻辑 quad、ANI 原点、动画 frame 几何、VM、存档或剧情时间；文字按实际 raster density 重新 shaping/rasterize glyph，再由根逆变换回逻辑位置。Family 帧的 `FrameInfo` 同时声明 physical `width/height` 与 logical `logical_width/logical_height`，Manager 和 Family 共用同一 viewport 计算完成输出与输入映射。无独立 SDK 的外部核心使用 logical=raster。

本地接续同时整合 KrKr、Siglus、Artemis、CMVS 和 Musica 的已有成果，全部使用新 Family API；统一使用 Musica 名称。新增核心的完成标准为真实代表流程，包括启动、连续剧情、媒体、选择或系统页、存读档和退出重开。FVP 与 Musica 仍各验证一条结局。

FamilyDescriptor 增加有界 typed 配置 schema（bool/integer/number/string/enum，分组和默认值）；OpenRequest 增加相同 schema 对应的 typed 配置值。字段 ID 唯一，未知键、重复键、类型/范围/枚举不匹配都在 open 前返回可定位错误。未提供值使用声明默认值。配置只在启动时生效，Manager 按核心/游戏持久化，核心继续验证。

TextReplacement 表示可选能力，未传 service 表示本次禁用，不应阻止支持正文翻译的核心启动。PCM capability 仍要求对应 sink。CPU 帧借用和音频有界/可取消写入不变。ABI 变更递增 fingerprint，旧插件明确重装，不增加旧 reader。动态库加载后在进程中保留，session 关闭取消所有 worker/服务调用；切换游戏不卸载库。

FVP 共用 RFVP VM、原生媒体和存档，仅补必要接口。Musica 使用 SDK，自有存档不覆盖原版；原版兼容不属于当前发布要求。SDK 不能依赖 VN 或强迫核心创建 RuntimeWorld。

全部适配核心采用最小必要修改，优先启用已有 GPU feature 和原生平台适配。Family API 的 CPU 最终帧借用只约束跨 ABI 交付方式，不要求 CPU 渲染：核心使用自己的 GPU 管线，交付时回读最终帧。不得为接入统一 Manager 复制一套核心渲染器。Windows Sandbox 通过 GPU 虚拟化执行视听测试；软件 adapter 明确失败，不能替代 GPU 验收。

所有核心诊断接入 Manager，不能只写在动态库内部的独立日志系统。Family API v7 在 descriptor/probe/open 前安装进程级 DiagnosticSink；适配层可选用 API crate 的 diagnostic-bridge feature，将 tracing 与 log 统一转发。Manager 仍是日志 sink、过滤和 flush 的唯一所有者。初始化冲突明确失败，不保留无日志的启动路径。桥不改变核心 GPU、平台和存档实现，也不要求依赖旧 SDK。



Rust Family 可选用 API crate 的 ProviderModule 复用 ABI 会话管理，FVP、Musica 和 Siglus 共用此实现。它直接适配现有 FamilyProvider/FamilySession，不引入引擎或 SDK 依赖，也不改变 Family ABI。一个模块只持有一个会话；打开、推进、帧借用和完整关闭共用互斥边界，关闭返回前不能重新打开。panic 后拒绝继续工作，但允许取回会话执行关闭；模块释放时也须关闭遗留会话。公共边界错误使用 ASTRA_EMU_FAMILY_SESSION_ACTIVE、SESSION、LOCK 和 PANIC 后缀，核心自身诊断保持原有代码，不保留旧 Family 私有边界错误别名。

### 外部核心与 SDK 的重构边界

Musica 的原生 stage 以完整资源序列、参考坐标、背景与立绘参数保存于 `runtime_state.v24`，替换丢失参数的旧图层表示。正常绘制与读档重建共用 Family Scene；未实现的序列或立绘语义必须明确失败，不能丢弃参数。v23 及更早状态直接拒绝，格式与诊断见 [Musica 脚本执行](../emu/musica/script-execution.md)。

Musica 的高清纹理替换只在私有 `MusicaProfile` 中声明 `texture_overrides` 映射：键是已挂载的 `musica:/` 原资源 URI，值是游戏目录内安全的 PNG 相对路径。启动时先验证原资源、ANI 原点和受支持的帧数，首次使用再解码原资源并按映射读取替换像素；替换像素只改变物理纹理，不改变脚本坐标、裁剪、命中区域、存档或 VM 时间。静态 PNG 和单帧 ANI 可以使用 PNG 替换，多帧 ANI、SQZ 及其他格式明确拒绝，不能只替换动画首帧后继续运行。缺失、越界、路径不安全、帧数不匹配或不支持的格式都返回诊断，不静默回退到原图。原生与替换像素使用同一个 SDK `TextureCache`，以不同身份键区分，随同一缓存的有界淘汰释放；替换素材只存在于 ignored 私有工作区。Family 负责 Musica 归档 URI 和 ANI 规则，SDK 只提供通用纹理帧与逻辑/物理尺寸类型。

吸收 `emu/krkr-hosted` 中外部核心 fork、最小适配提交和 submodule 的决策。来源提交 `dad452d2d` 将 RFVP、Siglus 改为 submodule，`bfc49f419` 引入 Artemis；这些提交用于确认来源，不整体移植旧 Family 接口。目标是保留完整上游历史，在固定上游基线上用一个适配提交承载必要差异，由主仓 gitlink 锁定精确提交。分支名只用于维护，不能替代提交固定。

Family descriptor、配置、日志桥和 ABI 转换放在主仓适配层。fork 只补嵌入入口、原生 GPU 输出、PCM 接口及必要生命周期能力，尽量保持上游平台入口可用。核心自身缺陷整理最小复现后交上游 issue，禁止附带商业素材、私有路径和存档；必要临时适配说明原因和删除条件。不以本次重构为由重写外部 VM、渲染器或媒体系统。

整理后的 fork 保留许可证、归属和上游基线，Family 的 `MODIFICATIONS.md` 记录每项差异。新增改动在独占本地分支合成单个适配提交并验证，再更新 submodule；不重写来源工作树或已发布历史。本地提交不等于已发布，推送仍在本轮授权范围之外。

Musica 和 CMVS 一起驱动 `astra-emu-sdk` 模块化。先对照 AstraEngine 现有文字、绘制、解码、混音与字节源实现，再提取两个核心共同需要的能力；格式解析、VM 和原生存档语义继续由核心拥有。SDK 按需组合，不依赖 VN、World/package/registry 或强制 EngineSession；不建立纯转发 provider，不为成熟外部核心增加 SDK 依赖。替换调用方后删除重复实现。

CMVS 私有配置 `astra.emu.cmvs.profile.v2` 用有序 `archives: [{ role, path }]` 声明原生归档优先级，role 必须唯一，路径仍由 SDK 限制在授权游戏目录。`mount_cmvs` 保留列表顺序，同名脚本沿该顺序查找；不得转换成排序映射。旧 v1 配置直接拒绝，使用者按预期优先级重写，不提供推测顺序的迁移。配置校验失败不得开始会话，也不能以另一归档次序重试；真实游戏代表流程仍是完成条件。

## 创作与编辑

.astra 是 Story/Scene/Sequence/角色预设/UI 的唯一创作来源。成熟 CST/AST 工具链保留注释/source map；布局保存作者元数据。高级逻辑是可信 Luau，保存显式状态，IO 通过平台宿主异步执行。

GPUI Editor 使用独立真实 GPU 预览窗口。seek 仅当前片段，重建不重复外部 IO。统一版本化编辑 API 服务 UI、ACP 和 MCP；自主/逐批确认模式都支持取消、冲突检查、批量撤销。ACP 的模型配置由外部 Agent 持有。

`astra-vn-editor::AuthoringWorkspace` 是编辑事务 owner。`EditBatch` 使用 project-relative `.astra` 路径、预期文档版本和 UTF-8 字节范围；多文档全部校验后原子应用。undo/redo 不复用旧版本。CST attribute editing 只替换原 value span，保留注释和 source ID。ACP 通过回合内的 loopback HTTP MCP 服务访问同一文档，随机令牌仅留在该回合；MCP batch 同时绑定客户端读取的 version 和 session generation。外部权限请求单独交给用户，不能替代编辑事务审批。逐批确认延迟提交，人工修改造成的版本冲突、取消和关闭后的迟到结果必须拒绝。具体入口见 [Editor](../../Editor/README.md)。

首轮预览复用 cook/package/bundle 和独立 Player 进程，由产品主路径间接持有 VnSession。Editor 负责版本绑定、构建取消、错误反馈与进程回收；该入口不等于内嵌 session 或片段 seek 已完成。

## 测试、迁移与交付

普通测试不依赖 Headless；视听测试按需启动宿主。错误、损坏数据、取消、保存恢复及资源释放均需测试。删除旧双轨时同步所有真实调用方，不删除仍有用的产品行为测试。

Headless 的原生解码器和 GPU 资源在会话专属线程的 current-thread executor 内创建、使用和释放；客户端只跨线程传递 typed command 与结果。启动握手异步返回初始化错误，不阻塞调用方 executor。性能模式在同一资源所属线程设置并恢复调度策略，不为使构建通过而给原生指针补 `unsafe Send`。

Root workspace 管共享/Engine/VN/Player/工具；Editor 与 Emulator 使用独立 workspace、lockfile 和产物。平台目标三桌面+Android，缺环境不声称通过。终之空本地转换私有包保留 Classic/Modern 37 路线，Windows 长流程、其余代表流程。旧内部 package/save 可重建，原商业存档必须保护。

### Musica 静态立绘

静态 PNG 立绘通过成熟 PNG reader 读取原生边界文本 `ol/ot/or/ob`，不自行实现 PNG 或像素裁剪。默认原点模式按资源左右边界计算中心，附加参数减去底部边界后才裁掉可见高度；`ot` 不直接加到屏幕坐标。使用 SDK 纹理缓存和公共 GPU 裁剪命令，不修改缓存像素。元数据与纹理缓存分别有界；损坏、重复或超限几何字段必须失败。同名 SQZ 的存在不能静默忽略，动画与非默认原点模式保持未完成。

公共离屏 GPU renderer 通过 `WgpuOffscreenRenderer::with_default_compositing` 选择未显式声明颜色空间的 Sprite、文字、矩形和 Mesh2D 命令的混合空间，默认仍为 LinearSrgb。MeshBatch2D 保留自身声明，同一帧混用两种空间明确失败。Musica 使用 EncodedSrgb，在 GPU 上按原生编码颜色值混合，复用现有纹理、文字和裁剪管线。

完整字体的覆盖声明可由 `astra_text::font_unicode_coverage(bytes, face_index)` 从字体字符映射生成，复用 cosmic-text 已有 skrifa，不新增解析器或依赖。返回有序、互不重叠的 Unicode 标量区间，排除缺失 glyph；字体或 face 无效、没有可用映射时明确失败。此接口不代替实际 shaping、缺字和 fallback 检查。Musica 使用该接口，避免手写区间遗漏日文标点与符号。文字错误只向 Manager 传递经过校验的诊断码，不透传正文。


共享 SceneCommand 提供 `PushPixelMask { bits }` / `PopPixelMask`：64 位遮罩按屏幕像素平铺为 8×8，最高位对应 (0,0)，最低位对应 (7,7)，不随场景变换移动；嵌套遮罩取交集，栈下溢或帧末未闭合明确失败。它限制作用域内绘制命令的片元覆盖，不改变资源上传及最终帧 FilterGraph。GPU shader 执行遮罩，CPU 测试 renderer 明确返回 `ASTRA_MEDIA_PIXEL_MASK_GPU_REQUIRED`，不展开像素裁剪或切换后端。命令追加到现有序列化枚举末尾，不改变既有 tag；使用新命令的运行端须同步更新。Director type 26 只生成一次入场场景，保留既有图案表、时序和显式场景恢复，遮罩命令不含外部 IO。

日志桥按用户决定取消内容脱敏和字段白名单，正常转发字符串、message、Debug/Display；数值保留 typed 值。`DiagnosticValue::Text` 最大 4096 bytes，事件最多 32 个唯一字段；超限、重复字段及非有限数值计入 `dropped_fields`。Debug 使用有界 formatter，避免先分配任意大小字符串。来源、级别和事件保持可定位，Manager 继续拥有 sink。Family API/ABI 当前为 v7，旧插件须同步重建安装，不保留旧脱敏模式。

Musica 接续直接采用 `codex/minori-runtime-followup` 已验证的完整机制，按依赖批量移植剩余实现。验证重点是新 Family API、共享 SDK/GPU 和 session 生命周期的整合回归，不重复原引擎语义研究。旧 Host/provider 层仍按新架构替换。

Musica 的历史回放和五组角色语音开关由 Family descriptor 声明，Manager 保存并在启动时传入。它们属于安装/游戏配置，不属于剧情存档；读档继续采用当前开关，关闭播放不删除历史语音关联，也不缩短脚本要求的语音时长等待。

SDK 可选 `video-ffmpeg` 复用 AstraMedia 增量解码器，解码器只在 worker 所属线程创建和释放；客户端每次只提交一个异步请求，结果批次受包数和字节双重限制，至多暂存一个未装入批次的包。seek 清空暂存并将新代次传递到每个媒体包，关闭丢弃旧结果并 join。PCM 调用方提交前检查整批帧数和包数余量，不能把音视频包总吞吐锁在宿主帧率。该能力不要求 Engine/VN session，不是 Family 电影播放的替代验收。

同一模块的 `PcmQueue` 接收共享 `AudioFramePacket` 与 PCM，供核心已有音频 worker 混入输出，不创建设备或线程。格式、包数、完整驻留分配和时间顺序有界；seek 重置必须增加代次并清空旧 PCM，迟到包明确拒绝。无数据时停止电影音频时钟；只有后续包声明的时间间隔可作为静音推进。调用方在 Host 接受混音后发布播放位置，暂停时不消费队列。

Musica 的 movie 状态与当前 Media wait 一一对应，播放位置更新须同时匹配 media id 和 fence id，且不得倒退。原生存档只保存资源、原生参数、等待标识和微秒位置；恢复前校验完整关联，运行资源由 Family 重建。Family 通过显式 `ffmpeg-vcpkg` feature 接入完整解码与共享 GPU/PCM 路径；未选择该 feature 时明确拒绝电影，不提供首帧或静音回退。

FFmpeg 增量解码按原生 resampler 的输出上界分配有界缓冲，用微秒级内部延迟校正输出 PTS，直到 flush 不再返回样本才结束。不得按输入帧数分配升采样输出，或用整秒延迟判定流已排空；相同规则适用于 Engine 与 SDK 消费者。

Musica 的用户音量及静音独立于剧情音频状态：BGM、voice 和 SE（含 se2/se3）复用 Kira 子音轨增益，原生声音的 volume/fade/cursor 保存原值；恢复采用当前 Manager 配置，并在首批 PCM 前生效。静音不跳过资源解码、时长校验或 VM 等待。

Musica 将来源的后台播放偏好放入 Family 启动配置：默认失焦暂停，显式启用才继续；窗口挂起优先于该偏好。剧情、演出、电影与音频共用暂停判定，焦点/挂起状态不写入剧情存档。失焦清除快进和待消费输入，恢复不积累暂停时长。

Musica 的文字阴影沿用来源的圆形偏移描边：正文、说话人与 backlog 默认使用半径 2、黑色 alpha 192，选择项不套用。SDK `TextSceneLayout::outline` 接收可选 `TextOutline`，复用前景 shaping 与 GPU 字形资源；半径只接受 1–8，透明颜色、坐标溢出与布局标识冲突明确失败。关闭阴影释放描边布局，不重复上传仍可见的前景字形。Manager 的 `text_shadow` 是启动偏好，读档保留当前设置，不进入剧情存档。

Musica 的来源编码探测按文件比较 CP932／GBK 严格解码错误行数，平局采用 Manager 启动首选；解析器继续拒绝选定编码下的损坏字节。启动首选属于会话配置，实际编码属于脚本及原生保存状态。脚本切换和读档不能把当前文件编码当作下个文件的首选，也不能复用上个文件的字体绑定；中日文字复用同一 SDK 字形资源生命周期。

Musica 的 `.include` 在统一脚本加载入口展开，启动、chain 和读档共用同一入口。目标限 scr 角色内的安全直接 `.sc` 文件名；保留来源的指令格式及补行尾行为。循环引用、超过 32 层、展开超过 16 MiB 或累计读取超过 64 MiB 明确失败。实际编码在展开后检测，保存中的脚本 hash 覆盖完整展开字节；读档不能只校验根脚本。

Musica 存档页沿用来源的 10 页×10 槽编号，手动存读档打开第 2 页（槽 20），翻页保留页内位置，槽焦点独立校验。存储 API 显式接收 0–99 的槽号；非法槽号在创建目录前失败，损坏文件和其他游戏身份的有效文件均禁止覆盖。原子替换仍只发生在当前槽文件，文件检查失败不得视为不存在。

Musica 原生存读档页复用 GPU Scene 和 SDK 文字，保持来源素材尺寸、槽位坐标与选择规则。保存卡片采用本地时间和 96×54 PNG，解码限制尺寸与分配预算；损坏卡片或旧容器明确失败，不当作空槽。手动保存剥离存读档页面状态，保留当前剧情等待；缩略图来自页面打开前的剧情帧。

终之空 Headless 路线测试沿途抽样截图，覆盖已显示正文、选择确认前和不同剧情状态的首段正文；前中后均匀取样，终局截图另行保留。终点黑场不能代表整条路线的画面结果，抽样截图也不能替代完整视听和真实 Player 验收。截图只留在 ignored 私有工作区。

NativeVN UI 帧复用同时检查逻辑模型版本与文字可见字数/总字数。呈现时钟独立推进，不能仅因逻辑步号不变而复用旧文字帧；逐字显示完成的观察值须对应实际提交的字形。静止页面继续复用既有 GPU 资源，不按帧序列化或复制完整 ViewModel。

Musica 快捷存档沿用来源的 `pc_line` 去重与槽 10–19 轮转。安装目录中的独立游标在保存成功后原子持久化，不进入剧情快照，读档不能倒转轮转位置。游标绑定游戏身份并检查 0–9 范围，损坏或异游戏文件明确失败且不覆盖；F9 读取游标对应的最近快捷槽，不扫描或猜测替代槽位。

Musica 路线解锁沿用来源分支的 TOHKA_CLEAR、AYAME_CLEAR、SUI_CLEAR、REN_CLEAR 白名单，仅脚本 setglobal 写入 1 时记录。解锁独立于剧情槽持久化，冷启动及读旧档合并保留；未知、重复、损坏或异游戏进度明确拒绝，读取失败不覆盖原文件。

Musica 原生启动模式使用 `MusicaLaunchMode::Direct/Title`。Title 会话的 end 返回原生标题，Direct 仍结束会话；标题菜单和恢复必须保留会话选择的启动方式，不恢复旧 Host semantic menu ABI。未接入的页面明确报错，不能静默开始剧情。


Family API v7 增加可选 SetFullscreen 窗口命令及 logical/raster FrameInfo，Manager 在 Slint 窗口线程执行输出 letterbox 与 pointer 逆映射，关闭会话恢复普通窗口。该命令不传 UI 类型或原生句柄；无窗口 Host 明确拒绝。Musica 原生设置通过同一通道应用与冷启动恢复全屏。

核心文件缺失、加载失败、descriptor/capability 不匹配或重复 `plugin_id` 时，Manager 按 `data/cores/` 中的文件显示诊断并继续加载其他唯一核心；冲突 ID 的全部文件禁用。SQLite 不记录插件路径，更新核心需替换文件并重启 Manager。不能因单个核心失败关闭管理界面。


### Player 显式静音测试输出

Windows bundled Player 接受一次性 `--test-null-audio` 参数。默认仍打开系统音频设备，失败返回诊断，不自动选择测试输出。测试参数不写入配置、package 或存档；窗口标题持续显示 `[TEST NULL AUDIO]`。测试输出复用 `NativeAudioQueue`，按采样率消费解码和 Kira 混音结果，保留暂停、恢复、播放时钟、队列背压、取消及关闭 worker 的路径，只替换设备输出。该模式不代表可听音频验收。其他平台与自动化子命令拒绝此参数。


实时 Player 调度每次最多消费四个到期逻辑 tick，保持原 deadline 与连续 tick 编号，剩余欠账留到下一次事件循环；短暂 GPU/IO 停顿不再触发致命 scheduler debt 错误。每批之间允许平台输入和退出，不能一次无限追赶、静默跳步或把迟到时间归零。初始化仍在固定时钟启动前完成；该调度变化不暂停或替代设备播放时钟。


NativeVN Player 保留启动时的逻辑舞台尺寸，窗口 resize 不改写剧情实体、摄像机或演出快照。实际 SceneFrame 使用窗口尺寸，并由公共 `Canvas2D` 对剧情与 Yakui UI 统一执行 aspect-fit 根变换及 viewport 裁剪，输出黑边；同一 viewport 逆映射鼠标和触控，黑边输入不送入游戏，accessibility bounds 映射到实际输出。恢复存档保持当前窗口尺寸，逻辑坐标来自剧情快照。Windows 与 Android 都经过共享 `NativeVnHostCommandSource`。


平台键盘事件使用 winit 的物理键内部名称（如 `Enter`、`F5`）与命名逻辑键（如 `Escape`、`ArrowDown`）；字符键保留实际字符。不得把 `Code(...)` 调试包装或 Enter 的控制字符交给 UI 绑定。Windows、Linux、macOS 和 Android 的适配点保持相同规则，设备验收分别记录。


Player 读档先校验候选舞台、转场身份、目标与转场源纹理及容量，再提交 RuntimeWorld；缺失资源或非法候选不得取消当前任务或改写当前游戏。转场两侧纹理共同受 GPU 驻留保护。图片预取请求及完成值携带 TaskScope 代次，成功读档后换代，旧失败不得污染新场景；不可变包缓存可复用，当前代失败仍返回诊断。退出先停止接单并取消队列，等待正在执行的解码，逐个 join 全部 worker。即使游玩、媒体关闭或资源释放失败，也必须继续执行会话和平台关闭；Drop 负责最后的 worker 回收。
### Android Player 启动与挂起边界

Activity 事件循环必须先于包读取启动。包读取分块让出事件循环，校验 worker 只持有不可变字节与取消标记；同次启动只进行一次完整 storage audit，已验证 reader 直接交给 Player，不再重复整包读取或 hash。读取界面显示真实字节数，后续阶段使用不定进度。取消和销毁必须停止读取、拒绝迟到结果并回收 worker；重试复用同一事件循环。

挂起停止权威推进和媒体时钟、暂停设备音频，保留渲染器和纹理而释放系统 surface。恢复重新绑定 surface、恢复设备与调度原点，不补跑后台欠账。短暂窗口失焦不等同于 Activity 挂起。正常结束与错误结束都执行媒体、预取、GPU、窗口和 host 清理，清理失败不得覆盖首个运行错误。


桌面与 native Player 启动共用存档目录读取逻辑。旧格式、损坏内容或错误身份的槽保留占用状态并禁止加载和覆盖，UI 显示不可用状态，其他槽和新游戏保持可用；目录/平台 IO 故障仍明确失败，不当作空槽。目录在应用缩略图和可加载状态前调用 `VnSession::validate_save`，复用实际恢复的 Runtime 包身份、seed、AwaitQueue 和 VN 状态检查；预检不修改 World 或取消任务。直接保存和缩略图准备同样检查写入资格。 用户选槽读取共用 `load_product_session`：候选提交前拒绝保留当前剧情、媒体和任务，将对应槽标为不可用并刷新当前页；提交后或设备恢复失败仍终止会话。Preview 显式恢复继续返回错误，不隐藏失败。native 会话在配置校验、窗口或 VN 创建失败时也必须关闭已启动 host。


F5/F9 由共享 VN 输入入口按 `quick_slot_id` 发起 typed 存读档请求。未声明快捷槽、空槽读取、受保护槽或按键 repeat 不发起 IO；桌面不得硬编码 `slot.quick`，native 入口使用相同规则。

用户读档在完整恢复剧情和媒体后，经 typed `ReturnSystem` 返回保存时的剧情等待，不停留在保存页或其他临时系统页。桌面、native 与 Headless 产品入口复用该行为。显式快照恢复和 Editor Preview checkpoint 仍完整恢复系统页，不能把用户读档收尾混入底层快照格式。

设置页的阅读模式和声音启用状态来自 `ConfigViewModel` 对 VN 权威系统状态的只读投影。Classic 按钮使用现有 `selected` 绑定，同时呈现持续选中外观和 accessibility selected 状态；键盘焦点移动不改变选择，点击只发 typed 请求，不在 UI 中保留第二份设置值。

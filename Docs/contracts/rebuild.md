# 全产品重构契约

本契约记录用户确认的新边界，实施状态见 [实施计划](../status/implementation-plan.md)。旧契约只在尚未迁移的实现中描述现状，冲突时以本契约为准。

## Engine 与共享库

公共 crate 使用 `astra-runtime`，公共会话使用 `EngineSession`；公共场景、任务、时钟、输入和呈现不得归入名为 AstraVNRuntime 的层。`VnSession` 持有 VN 剧情和系统状态，`VnRuntime` 名称仅用于 VN 业务。迁移必须调整实际所有权及调用方，不增加旧名称兼容别名。

共享文字、图像、绘制、解码、混音与字节源可不创建 World/package/registry 而使用。Engine/VN 与 Musica 是实际消费者。NativeVN 以普通 Rust typed API 组合，FSM 不作为唯一修改入口；固定 60 Hz 逻辑和独立呈现共用明确生命周期。任务具有句柄、作用域、完成/取消/失败结果；旧代次结果拒绝。存档保存显式状态，删除通用回放和帧内全量事务。

`astra-vn::VnSession` 是 NativeVN 的 typed 产品入口，持有剧情、变量和系统状态，并组合 `astra-runtime::EngineSession`。EngineSession 创建和持有 RuntimeWorld、任务作用域及逻辑步生命周期；VN 在修改剧情前校验步序、seed 和恢复模式。关闭会话取消其任务，不影响其他会话。保存仍使用原有 SaveBlob 容器，读取失败保留当前状态。

NativeVN 的原生宿主与 Headless 共用物理输入到 UI request/completion 的路径。快捷存档成功后由共享完成回调更新槽目录，Headless 不单独截获 F5/F9。Advance 只在未打开系统页、作品允许跳过且当前等待为 MovieEnd 时完成匹配的影片 fence；后台继续播放的影片不受影响。取消对应媒体任务后，媒体宿主关闭 decoder，旧请求返回的帧拒绝提交。删除 Headless 专用的 `skip_active_videos` 入口；调用方使用既有物理输入接口，不新增存档格式或兼容层。

旧 `astra-vn-runtime-provider` crate、NativeVnRuntimeProvider/Factory、FFI 转换和会话 map 已删除。Player、CLI、发布检查改用 `astra-vn`；静态包契约随后移除了 `native_vn_descriptor()` 和剩余 gameplay/UI 动态 ABI 消费链；机制 provider 的显式绑定保留。

## EMU Family API

现行 [Family API v13](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md) 使用 abi_stable、固定 owner 和不可变 CPU frame lease；epoch/generation 区分 Ready/Pending/Unchanged。PCM 查询区分已接纳/已播放源帧及 DeviceTimestamp/TestConsumption，不支持时明确拒绝查询。可选能力关闭不阻止基础播放。

窗口采用 typed SetFullscreen/SetTitle/SetIcon/SetPointer，模态服务使用有界 typed 请求。Host 在窗口线程应用、关闭恢复普通窗口与 Manager 标题/图标；无窗口 Host 明确拒绝。尺寸、字段和响应限制以 API 契约为准，不传 UI 类型或 native handle。

Manager headless 使用现有 `astra-observability` 的有界日志，与本次输出图像放在一起：将输出文件扩展名替换为 `.diagnostics` 作为日志目录。日志不写入游戏目录或存档，不属于 package；测试结束后按临时产物规则清理。

共享音频的流式解码只将解码器明确返回的流结束视为完成。`SymphoniaAudioStreamDecoder` 遇到意外 EOF 返回 `ASTRA_AUDIO_STREAM_TRUNCATED_INPUT`；已输出的有效 PCM 前缀不代表整段播放成功，调用方须终止失败的播放请求。正常完整流和解码预算规则不变。

GPU 回读后的 CPU 存储可以在进程内保留共享所有权。`OwnedPixelBuffer::from(Arc<[u8]>)` 复用原像素分配，`make_mut_for_update` 保持写时复制；`TextureFrame` 校验尺寸和完整性。Family 以不可变 CPU lease 交付，Host 保留 owner 到上传完成，不要求同步复制，也不把 GPU 对象放进跨线程 lease。

`WgpuOffscreenRenderer` 的 Rust 构造入口显式接收 graphics owner 持有的 `wgpu::Instance`，不再按 renderer 隐式创建 Instance。Musica 的持久 provider 持有 Instance，普通启动和恢复候选共用它；Headless factory 的克隆共享 Instance，使用 `HeadlessThreadOwner` 时由该 owner 跨 session 保留。原生 backend 和 Windows StaticDxc 配置保持不变，每个 renderer 仍独立选择并验证 adapter policy、创建 device/queue 和场景资源。关闭 session 释放自身 GPU 资源并取消、等待其 worker，不能缓存整个 renderer 或 device 冒充释放。Instance 随显式 owner 结束而释放，不用生产全局静态缓存，也不保证同一进程无限次销毁和重建整个 graphics owner。旧的无 Instance 构造调用直接迁移，不保留兼容入口；错误保留原诊断 code 和安全 operation，不增加软件 fallback 或失败重试。

公共 `astra-media-core` 的 `Extent2D`/`Canvas2D` 负责逻辑舞台到 raster canvas 的比例校验、根变换和坐标映射，并复用 `RectI`、`Transform2D`、`TextureFrame`、`SceneCommand`。四个空间必须分开：逻辑舞台决定 VM、存档、时间和动画几何；资产保存物理像素并显式声明逻辑尺寸及格式原点；raster canvas 是 GPU 目标；窗口输出由 Host 做 letterbox 和 pointer 逆映射。`astra-emu-sdk` 只提供薄语义 `StageCanvas` 与 `TextureAsset`，不依赖 VN；AstraVN presentation 直接消费同一 `Canvas2D`，作为第二个真实调用方。

Musica 的产品目标为固定 `1280×720` 逻辑舞台及 `1.0/1.5/2.0/3.0` render scale。当前实现仍暴露 `logical_width/logical_height` 与 `render_width/render_height`，支持更广的逻辑/raster 尺寸；包括 eden 旧 1024×640 的整合尚未收敛，不能将目标写成已实现。现行 Canvas 以统一 scale 和居中 aspect-fit viewport 输出，viewport 外指针不进入游戏；配置/预算无效明确失败。资产密度不能改变逻辑 quad、ANI 原点、动画几何、VM、保存或时间，文字按 raster density 排版后映回逻辑坐标。尺寸契约收敛须保留这些不变量及已有存档身份校验，不通过静默缩放修改游戏语义。

当前整合 FVP、Siglus、Artemis、CMVS 和 Musica，KrKr 按用户决定跳过本批适配，保留原成果。统一使用 Musica 名称。新增核心验证真实代表流程，包括启动、连续剧情、媒体、选择/系统页、存读档和退出重开；FVP/Musica 最终各验证一条结局。

FamilyDescriptor 增加有界 typed 配置 schema（bool/integer/number/string/enum，分组和默认值）；OpenRequest 增加相同 schema 对应的 typed 配置值。字段 ID 唯一，未知键、重复键、类型/范围/枚举不匹配都在 open 前返回可定位错误。未提供值使用声明默认值。配置只在启动时生效，Manager 按核心/游戏持久化，核心继续验证。

TextReplacement 是可选能力，未传 service 表示本次禁用；PCM capability 仍要求对应 sink。CPU lease 保持不可变，音频写入有界且可取消。ABI 变更递增 fingerprint，插件统一重建，不加兼容 reader。动态库驻留至进程退出，session 关闭取消并等待 worker/服务调用；切换游戏不卸载库。

FVP 共用 RFVP VM、原生媒体和存档，仅补必要接口。Musica 使用 SDK，自有存档不覆盖原版；原版兼容不属于当前发布要求。SDK 不能依赖 VN 或强迫核心创建 RuntimeWorld。

成熟核心改动严格限必要嵌入钩子，复用原 GPU/媒体/平台实现；CPU lease 约束的是交付，不要求 CPU 渲染。产品硬件要求由显式嵌入配置或 Family 验证，不改变上游默认平台策略。实际视听使用硬件 GPU，Sandbox 可使用 GPU 虚拟化。

核心诊断在 descriptor/probe/open 前接入 Manager。动态适配层安装共享日志桥，静态核心复用宿主订阅器；Manager 是日志 sink、级别过滤和 flush 的所有者。桥初始化冲突阻止加载。字符串、message、Debug/Display 正常有界转发，数值保留 typed 值，超限计入 dropped_fields；字段不经白名单或内容脱敏。

Rust Family 可用 API crate 的 ProviderModule 管理 ABI 会话，不依赖 Engine/SDK。一个模块一个会话，由同一 owner 串行创建、推进、获取帧和完整关闭；关闭返回前不重开。panic 后拒绝继续工作，但允许取回会话清理；模块释放也关闭遗留会话。公共边界错误保留 SESSION_ACTIVE、SESSION、LOCK、PANIC 等诊断，核心错误保持原代码。

### 外部核心与 SDK 的重构边界

Musica 的原生 stage 以完整资源序列、参考坐标、背景与立绘参数保存于 `runtime_state.v24`，替换丢失参数的旧图层表示。正常绘制与读档重建共用 Family Scene；未实现的序列或立绘语义必须明确失败，不能丢弃参数。v23 及更早状态直接拒绝，格式与诊断见 [Musica 脚本执行](../emu/musica/script-execution.md)。

Musica 的高清纹理替换只在私有 `MusicaProfile` 中声明 `texture_overrides` 映射：键是已挂载的 `musica:/` 原资源 URI，值是游戏目录内安全的 PNG 相对路径。启动时先验证原资源、ANI 原点和受支持的帧数，首次使用再解码原资源并按映射读取替换像素；替换像素只改变物理纹理，不改变脚本坐标、裁剪、命中区域、存档或 VM 时间。静态 PNG 和单帧 ANI 可以使用 PNG 替换，多帧 ANI、SQZ 及其他格式明确拒绝，不能只替换动画首帧后继续运行。缺失、越界、路径不安全、帧数不匹配或不支持的格式都返回诊断，不静默回退到原图。原生与替换像素使用同一个 SDK `TextureCache`，以不同身份键区分，随同一缓存的有界淘汰释放；替换素材只存在于 ignored 私有工作区。Family 负责 Musica 归档 URI 和 ANI 规则，SDK 只提供通用纹理帧与逻辑/物理尺寸类型。

外部核心保留完整上游历史，整合目标为上游 HEAD 加一个必要适配提交。所有 fork 的对外适配分支统一命名为 `astra`，当前与后续核心适配均沿用该名称；`.gitmodules` 的跟踪分支与 Family metadata 同步声明 `astra`。主仓仍由 `Emulator/ThirdParty` submodule 固定精确 revision，分支名不能代替 gitlink，不复制核心源码。

descriptor、配置、日志桥、ABI 转换与产品策略放在 Family。核心只提供必要入口、原生输出和生命周期通知。游戏暴露的引擎功能缺陷独立提交上游 issue/PR，附自制最小复现；合并前标明依赖，不能长期混称为适配。适配自身回归由适配修正，不通过重写 VM/renderer/媒体或回退必要功能缩小 diff。

整理使用独占工作树和 `astra` 适配分支，保留许可证和 MODIFICATIONS 中的基线/差异，不改写来源或已发布历史。已授权发布上述上游功能修复；产品、完整适配分支和其他外发仍按各自授权执行。当前状态、审查结论及执行顺序只在 [status](../status/README.md) 维护。

Musica 和 CMVS 一起驱动 `astra-emu-sdk` 模块化。先对照 AstraEngine 现有文字、绘制、解码、混音与字节源实现，再提取两个核心共同需要的能力；格式解析、VM 和原生存档语义继续由核心拥有。SDK 按需组合，不依赖 VN、World/package/registry 或强制 EngineSession；不建立纯转发 provider，不为成熟外部核心增加 SDK 依赖。替换调用方后删除重复实现。

CMVS 私有配置 `astra.emu.cmvs.profile.v2` 用有序 `archives: [{ role, path }]` 声明原生归档优先级，role 必须唯一，路径仍由 SDK 限制在授权游戏目录。`mount_cmvs` 保留列表顺序，同名脚本沿该顺序查找；不得转换成排序映射。旧 v1 配置直接拒绝，使用者按预期优先级重写，不提供推测顺序的迁移。配置校验失败不得开始会话，也不能以另一归档次序重试；真实游戏代表流程仍是完成条件。

## 创作与编辑

.astra 是 Story/Scene/Sequence/角色预设/UI 的唯一创作来源。成熟 CST/AST 工具链保留注释/source map；布局保存作者元数据。高级逻辑是可信 Luau，保存显式状态，IO 通过平台宿主异步执行。

GPUI Editor 使用独立真实 GPU 预览窗口。seek 仅当前片段，重建不重复外部 IO。统一版本化编辑 API 服务 UI、ACP 和 MCP；自主/逐批确认模式都支持取消、冲突检查、批量撤销。ACP 的模型配置由外部 Agent 持有。

`astra-vn-editor::AuthoringWorkspace` 是编辑事务 owner。`EditBatch` 使用 project-relative `.astra` 路径、预期文档版本和 UTF-8 字节范围；多文档全部校验后原子应用。undo/redo 不复用旧版本。CST attribute editing 只替换原 value span，保留注释和 source ID。ACP 通过回合内的 loopback HTTP MCP 服务访问同一文档，随机令牌仅留在该回合；MCP batch 同时绑定客户端读取的 version 和 session generation。外部权限请求单独交给用户，不能替代编辑事务审批。逐批确认延迟提交，人工修改造成的版本冲突、取消和关闭后的迟到结果必须拒绝。具体入口见 [Editor](../../Editor/README.md)。

首轮预览复用 cook/package/bundle 和独立 Player 进程，由产品主路径间接持有 VnSession。Editor 负责版本绑定、构建取消、错误反馈与进程回收；该入口不等于内嵌 session 或片段 seek 已完成。

## 测试、迁移与交付

普通测试不依赖 Headless；视听测试按需启动宿主。错误、损坏数据、取消、保存恢复及资源释放均需测试。删除旧双轨时同步所有真实调用方，不删除仍有用的产品行为测试。

Headless 的原生解码器和 GPU 资源在会话专属线程的 current-thread executor 内创建、使用和释放；客户端只跨线程传递 typed command 与结果。启动握手异步返回初始化错误，不阻塞调用方 executor。性能模式在同一资源所属线程设置并恢复调度策略，不为使构建通过而给原生指针补 `unsafe Send`。 CLI 的 run/serve 还持有 HeadlessThreadOwner：正常会话先走 shutdown，错误或取消收尾时取消剩余 Host loop 并 join 专属线程，确认 executor/native resource 析构完成后才返回到进程退出。with_thread_owner 绑定本地线程和 graphics Instance 的生命周期，不改变公共 PlatformHostClient 契约或错误结果。

Root workspace 管共享/Engine/VN/Player/工具；Editor 与 Emulator 使用独立 workspace、lockfile 和产物。平台目标三桌面+Android，缺环境不声称通过。终之空本地转换私有包保留 Classic/Modern 37 路线，Windows 长流程、其余代表流程。旧内部 package/save 可重建，原商业存档必须保护。

### Musica 静态立绘

静态 PNG 立绘通过成熟 PNG reader 读取原生边界文本 `ol/ot/or/ob`，不自行实现 PNG 或像素裁剪。默认原点模式按资源左右边界计算中心，附加参数减去底部边界后才裁掉可见高度；`ot` 不直接加到屏幕坐标。使用 SDK 纹理缓存和公共 GPU 裁剪命令，不修改缓存像素。元数据与纹理缓存分别有界；损坏、重复或超限几何字段必须失败。同名 SQZ 的存在不能静默忽略，动画与非默认原点模式保持未完成。

公共离屏 GPU renderer 通过 `WgpuOffscreenRenderer::with_default_compositing` 选择未显式声明颜色空间的 Sprite、文字、矩形和 Mesh2D 命令的混合空间，默认仍为 LinearSrgb。MeshBatch2D 保留自身声明，同一帧混用两种空间明确失败。Musica 使用 EncodedSrgb，在 GPU 上按原生编码颜色值混合，复用现有纹理、文字和裁剪管线。

完整字体的覆盖声明可由 `astra_text::font_unicode_coverage(bytes, face_index)` 从字体字符映射生成，复用 cosmic-text 已有 skrifa，不新增解析器或依赖。返回有序、互不重叠的 Unicode 标量区间，排除缺失 glyph；字体或 face 无效、没有可用映射时明确失败。此接口不代替实际 shaping、缺字和 fallback 检查。Musica 使用该接口，避免手写区间遗漏日文标点与符号。文字错误只向 Manager 传递经过校验的诊断码，不透传正文。


共享 SceneCommand 提供 `PushPixelMask { bits }` / `PopPixelMask`：64 位遮罩按屏幕像素平铺为 8×8，最高位对应 (0,0)，最低位对应 (7,7)，不随场景变换移动；嵌套遮罩取交集，栈下溢或帧末未闭合明确失败。它限制作用域内绘制命令的片元覆盖，不改变资源上传及最终帧 FilterGraph。GPU shader 执行遮罩，CPU 测试 renderer 明确返回 `ASTRA_MEDIA_PIXEL_MASK_GPU_REQUIRED`，不展开像素裁剪或切换后端。命令追加到现有序列化枚举末尾，不改变既有 tag；使用新命令的运行端须同步更新。Director type 26 只生成一次入场场景，保留既有图案表、时序和显式场景恢复，遮罩命令不含外部 IO。


Musica 接续直接采用 `codex/minori-runtime-followup` 已验证的完整机制，按依赖批量移植剩余实现。验证重点是新 Family API、共享 SDK/GPU 和 session 生命周期的整合回归，不重复原引擎语义研究。旧 Host/provider 层仍按新架构替换。

Musica 的历史回放和五组角色语音开关由 Family descriptor 声明，Manager 保存并在启动时传入。它们属于安装/游戏配置，不属于剧情存档；读档继续采用当前开关，关闭播放不删除历史语音关联，也不缩短脚本要求的语音时长等待。

SDK 可选 `video-ffmpeg` 复用 AstraMedia 增量解码器，解码器只在 worker 所属线程创建和释放；客户端每次只提交一个异步请求，结果批次受包数和字节双重限制，至多暂存一个未装入批次的包。seek 清空暂存并将新代次传递到每个媒体包，关闭丢弃旧结果并 join。PCM 调用方提交前检查整批帧数和包数余量，不能把音视频包总吞吐锁在宿主帧率。该能力不要求 Engine/VN session，不是 Family 电影播放的替代验收。

同一模块的 `PcmQueue` 接收共享 `AudioFramePacket` 与 PCM，供核心已有音频 worker 混入输出，不创建设备或线程。格式、包数、完整驻留分配和时间顺序有界；seek 重置必须增加代次并清空旧 PCM，迟到包明确拒绝。无数据时停止电影音频时钟；只有后续包声明的时间间隔可作为静音推进。调用方在 Host 接受混音后发布播放位置，暂停时不消费队列。

Musica 的 movie 状态与当前 Media wait 一一对应，播放位置更新须同时匹配 media id 和 fence id，且不得倒退。原生存档只保存资源、原生参数、等待标识和微秒位置；恢复前校验完整关联，运行资源由 Family 重建。Family 通过显式 `ffmpeg-vcpkg` feature 接入完整解码与共享 GPU/PCM 路径；未选择该 feature 时明确拒绝电影，不提供首帧或静音回退。

FFmpeg 增量解码按原生 resampler 的输出上界分配有界缓冲，用微秒级内部延迟校正输出 PTS，直到 flush 不再返回样本才结束。不得按输入帧数分配升采样输出，或用整秒延迟判定流已排空；相同规则适用于 Engine 与 SDK 消费者。

Musica 的用户音量及静音独立于剧情音频状态：BGM、voice 和 SE（含 se2/se3）复用 Kira 子音轨增益，原生声音的 volume/fade/cursor 保存原值；恢复采用当前 Manager 配置，并在首批 PCM 前生效。静音不跳过资源解码、时长校验或 VM 等待。

Musica 将来源的后台播放偏好放入 Family 启动配置：默认失焦暂停，显式启用才继续；窗口挂起优先于该偏好。剧情、演出、电影与音频共用暂停判定，焦点/挂起状态不写入剧情存档。失焦清除快进和待消费输入，恢复不积累暂停时长。

Musica 的文字阴影沿用来源的圆形偏移描边：正文、说话人与 backlog 默认使用半径 2、黑色 alpha 192，选择项不套用。SDK `TextSceneLayout::outline` 接收可选 `TextOutline`，复用前景 shaping 与 GPU 字形资源；半径只接受 1–8，透明颜色、坐标溢出与布局标识冲突明确失败。关闭阴影释放描边布局，不重复上传仍可见的前景字形。Manager 的 `text_shadow` 是启动偏好，读档保留当前设置，不进入剧情存档。

SDK `TextScene::layout_shared` 只读访问同一字体 provider/cache 的布局结果，原生核心可使用真实 glyph advance。测量不创建 GPU 资源或可见区域；`frame` 保持资源上传、绘制及释放的唯一入口，两者共用 Error/Blocking 诊断。该 Rust API 不改变 Family ABI；既有消费者无迁移要求。SDK 测试覆盖缓存复用、测量无资源副作用和后续正常上传，CMVS 消费者另测固定与比例排版。

Musica 的来源编码探测按文件比较 CP932／GBK 严格解码错误行数，平局采用 Manager 启动首选；解析器继续拒绝选定编码下的损坏字节。启动首选属于会话配置，实际编码属于脚本及原生保存状态。脚本切换和读档不能把当前文件编码当作下个文件的首选，也不能复用上个文件的字体绑定；中日文字复用同一 SDK 字形资源生命周期。

Musica 的 `.include` 在统一脚本加载入口展开，启动、chain 和读档共用同一入口。目标限 scr 角色内的安全直接 `.sc` 文件名；保留来源的指令格式及补行尾行为。循环引用、超过 32 层、展开超过 16 MiB 或累计读取超过 64 MiB 明确失败。实际编码在展开后检测，保存中的脚本 hash 覆盖完整展开字节；读档不能只校验根脚本。

Musica 存档页沿用来源的 10 页×10 槽编号，手动存读档打开第 2 页（槽 20），翻页保留页内位置，槽焦点独立校验。存储 API 显式接收 0–99 的槽号；非法槽号在创建目录前失败，损坏文件和其他游戏身份的有效文件均禁止覆盖。原子替换仍只发生在当前槽文件，文件检查失败不得视为不存在。

Musica 原生存读档页复用 GPU Scene 和 SDK 文字，保持来源素材尺寸、槽位坐标与选择规则。保存卡片采用本地时间和 96×54 PNG，解码限制尺寸与分配预算；损坏卡片或旧容器明确失败，不当作空槽。手动保存剥离存读档页面状态，保留当前剧情等待；缩略图来自页面打开前的剧情帧。

终之空 Headless 路线测试沿途抽样截图，覆盖已显示正文、选择确认前和不同剧情状态的首段正文；前中后均匀取样，终局截图另行保留。终点黑场不能代表整条路线的画面结果，抽样截图也不能替代完整视听和真实 Player 验收。截图只留在 ignored 私有工作区。

NativeVN UI 帧复用同时检查逻辑模型版本与文字可见字数/总字数。呈现时钟独立推进，不能仅因逻辑步号不变而复用旧文字帧；逐字显示完成的观察值须对应实际提交的字形。静止页面继续复用既有 GPU 资源，不按帧序列化或复制完整 ViewModel。

Musica 快捷存档沿用来源的 `pc_line` 去重与槽 10–19 轮转。安装目录中的独立游标在保存成功后原子持久化，不进入剧情快照，读档不能倒转轮转位置。游标绑定游戏身份并检查 0–9 范围，损坏或异游戏文件明确失败且不覆盖；F9 读取游标对应的最近快捷槽，不扫描或猜测替代槽位。

Musica 路线解锁沿用来源分支的 TOHKA_CLEAR、AYAME_CLEAR、SUI_CLEAR、REN_CLEAR 白名单，仅脚本 setglobal 写入 1 时记录。解锁独立于剧情槽持久化，冷启动及读旧档合并保留；未知、重复、损坏或异游戏进度明确拒绝，读取失败不覆盖原文件。

Musica 原生启动模式使用 `MusicaLaunchMode::Direct/Title`。Title 会话的 end 返回原生标题，Direct 仍结束会话；标题菜单和恢复必须保留会话选择的启动方式，不恢复旧 Host semantic menu ABI。未接入的页面明确报错，不能静默开始剧情。



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

Yakui 的 `clip_rect_points` 必须以成对 `PushClip`/`PopClip` 进入共享 Scene2D，保持逻辑坐标并由 Canvas 统一变换。单独绘制的 AstraText 使用真实 Yakui `clipped_by` 祖先交集；布局层把整数范围投影为现有只读 semantic text properties 的 `text.clip.x/y/width/height`，Player 不从父控件尺寸猜测裁剪。空交集不绘文字，缺失部分字段、非有限或越界坐标拒绝；相邻控件不能继承前一控件的裁剪。

NativeVN 宿主通过 prepare_ui_input 处理物理输入。只有剧情实际进入系统页或提出快捷保存时，才在该批首个呈现命令之前插入 CaptureSurface；宿主消费 Captured 结果后再处理保存请求。键盘、鼠标、导航与辅助功能入口使用同一状态转换判断，系统页内部切换不覆盖 gameplay thumbnail。直接 dispatch_ui_event 仍供不负责存档缩略图的预览/逻辑调用者使用。迁移宿主时须同时消费截图结果，不能仅替换方法名；回归检查截图在呈现前、序列递增、系统页内不重复捕获，并用真实保存页检查缩略图。

Windows 新建窗口在首次显示前按所在显示器的 work area 放置，外框不得因系统级联位置挡住底部操作；可用区域不足时缩小物理客户区，剧情逻辑尺寸仍由共享 Canvas 管理。桌面和 native Player 的 `current_save_timestamp` 共用本地墙钟显示，时区不可得时明确附加 UTC 标识；Headless 保留确定性的 T+ 时间。既有槽的显示字符串不改写。

## 静态游戏 UI 包格式

NativeVN 使用编译期组合的 Yakui。Cook、bundle 和 Player 不再接收动态 UI component artifact、signer allowlist 或组件子进程配置；旧 `ui_component`、`component_slot` 源码需改为标准 widget。UI blueprint 为 `astra.ui_blueprint_bundle.v2`，compiled project 为 v4，package root 为 v3。旧包重新 Cook；读取时拒绝旧 root，不保留 reader fallback。作品 hash、section 完整性和存档损坏校验保持有效。

## 独立 Engine 会话构造

`EngineSession::new(RuntimeConfig)` 创建无包、普通执行模式的会话；嵌入者无需选择 provider、完整性模式或 worker 数。确实需要包绑定或调度配置的产品先配置 `RuntimeWorld`，再通过 `EngineSession::from_world` 交出所有权。NativeVN 的会话身份使用自身字符串，跨 ABI 的 `GameRuntimeSessionId` 不再进入静态 VN session 或 Player 存档类型。字符串编码和值保持不变，作品、包、schema 和内容完整性校验不变；旧 v7/v8 保存格式仍拒绝。

## 静态 gameplay 打包边界

本轮 NativeVN 随 Player 编译，目标声明不再选择 gameplay runtime 或 UI provider。`astra.target_manifest.v3` 删除 `runtime_provider`/`ui_provider`；`astra.provider_policy.v3` 只保存真实呈现、VFS、音频和解码等机制的显式绑定，不保存 `ProductRuntimeDescriptor`，不要求 `game_runtime_provider` slot。旧 target/policy 版本须重新生成；不提供别名或 reader 回退。

Package reader 从明确选中的 packaged game target 与 package profile 建立静态产品上下文，不通过动态 gameplay descriptor 选择实现。它继续校验容器版本、section 唯一性、内容 hash、target/profile eligibility、VFS namespace 与 provider capability/fingerprint。NativeVN package 的必要语义和 release 检查由 NativeVN 的静态契约决定，不依赖包内字符串声明哪些检查应该运行。

旧动态 gameplay host、FFI descriptor 和验证选择对象在最后一个真实消费者迁移后删除。普通 Engine session 不因此依赖 package；平台 renderer、audio output、mixer、decode 和 VFS 的真实显式选择不被移除。验证覆盖无 gameplay provider 配置的 Cook/package/Player 会话、缺失和冲突机制绑定、旧 schema 拒绝、包损坏及产品存读档。


### Luau 显式 IO 延续

Native `VnSession` 通过 `bind_policy_io` 授予具名逻辑资源及读取限额，再由 `evaluate_policy` 执行可信策略。当前 typed 操作为 `PolicyReadText`，返回 UTF-8 文本或 `PolicyIoFailure`；包或脚本不能自行授予权限。脚本用 `astra.host.read_text(key, uri)` 获得 `(text, status)`，在后续调用中读取已提交结果，用 `astra.snapshot` 保存显式阶段。`cancel` 取消请求，`release` 释放已结束记录；key 复用不会复用请求序号。每会话最多保留 64 个请求，每次读取最多 1 MiB；不开放原生文件路径或隐式 IO。

每个 pending 请求对应 Runtime `AwaitToken`。Session 按请求序号调度一个可取消 async worker，完成消息只能在成功的固定 tick 中通过原 scope 的 completion handle 进入状态。Host 必须提供取消安全的 Future，不能执行阻塞调用或遗留 detached worker。关闭、显式取消和成功恢复均取消并 join worker；损坏恢复候选在取消旧工作之前拒绝。Web 暂不提供原生 worker，返回 `ASTRA_POLICY_HOST_IO_UNSUPPORTED`，不静默执行同步 IO。

`astra.vn.session_policy.v1` 保存策略值、请求及 AwaitToken 对应关系，不保存 VM、coroutine、执行栈、宿主 grant 或句柄。恢复保留完成结果，将 pending 明确转为 interrupted 并清除相应 await，不自动重发外部请求。缺失的新 session-policy component、版本错误、无效序号、超限结果或 await 不一致均拒绝；旧槽不迁移。读取已保存结果不需要再次授予权限；发起新请求仍需当前宿主授权。

实现沿用现有 `mlua` VM 与 Runtime `TaskScope`。`mlua` 的 [async 支持](https://docs.rs/crate/mlua/0.10.5/source/README.md) 依赖 coroutine；本接口采用显式延续，避免将 coroutine 误当作可恢复状态。取消遵循 [CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html) 的 Future drop 约定。平台 IO adapter 由 embedding host 显式提供；这一批没有自动给 Packaged Player 新增磁盘或网络权限。


### 包内 Luau 注册动作

剧情动作复用 `ExtensionCommandDescriptor`、`ExtensionValue` 和现有 `.astra` 扩展命令语法，不新增 policy 关键字。项目在 `nativevn.policy_actions` 中显式列出项目内 `.luau` entry 与 `read_text` asset ID；Cook 在受限 VM 中执行注册，校验 descriptor、参数、重复命令、entry 和源码 hash，再将绑定写入 package。运行时只调用与已编译命令完全匹配的 `PolicyActionBinding`；注册得到的 handler 保留在本次 VM，不能只记录注册日志。

模块顶层只负责注册。handler 返回 `PolicyActionOutcome::Complete` 或 `Await { request_key }`，不能使用 boolean 隐式表示等待。每次剧情执行分配单调递增的 action instance，IO key 归该实例所有；同一 source ID 在循环中再次执行不能取得前一次结果。Await 只能引用当前实例实际存在的 pending 请求，未知、已结束或其他实例的 key 均拒绝。

handler 可以通过模块局部变量捕获 `astra` table；注册结束后，该 table 原位获得当前动作的运行 API，资源授权和 IO key 仍受当前实例约束。顶层不能调用 host 或 mutation API。注册函数只在模块加载阶段开放；捕获 `astra.command` 或 `register` 函数也不能在 handler 内继续注册。每次调用重建 VM，不跨实例保留 API table 或闭包。

handler 从头调用，VM 与执行栈不保存。变量写入先暂存，只有 Complete 才一次提交给 VN 权威状态；Await 丢弃本次 gameplay 写入，仅保留显式 continuation state 与 IO 状态。不能将 command trace 当作已经执行的解锁或其他效果，未接入的 effect 必须报错。`PolicyActionContinuation` 保存 source ID、实例序号、typed 参数、等待 key 与显式 state；复用现有 VN wait 和 IO AwaitToken，不另建调度器。固定 tick 接收 IO completion，再调用 handler；普通物理输入不能跳过动作等待。

保存组件使用 `astra.vn.session_policy.v2`，旧 v1 明确拒绝，不迁移或覆盖旧槽。恢复验证 continuation、命令绑定、请求归属和 await 一致性后，才替换旧会话。pending 转为 Interrupted，不自动重新 dispatch；恢复后 handler 读取 Interrupted 并由显式分支完成或提出新的请求。损坏候选不取消旧会话，不覆盖旧槽。关闭取消并 join worker，迟到 completion 不得提交。

Packaged Player 的 ReadText adapter 只读取显式授权且满足当前 VFS binding/target/profile 的包内 UTF-8 文本。启动阶段完成 package section/hash/长度验证与有界预载，计入已有资源预算；取消或失败不交付半初始化会话。运行阶段提供内存文本的可取消读取，不宣称实时磁盘 IO；包内授权不包含任意文件路径或网络。底层同步 package reader 不能放进 async poll 后声称可取消，启动通过现有事件流处理关闭，取消启动作用域并 join 所有者线程；每个有界 section 读取前后检查取消，不能中断正在执行的同步磁盘调用。现有 tracing 记录已加载条目数与字节数，不新增事件协议。

本批检查须覆盖真实 `.astra` → Cook → package → Player-VN 等待与恢复后再保存、同 source 重复执行、Await 前写入只在 Complete 提交一次、非法等待 key、阻塞 host 关闭和迟到结果。局部 VM 测试不能替代产品调用链；桌面 GUI 与真实设备验收单独报告。


包内动作保存在 `vn.policy_actions` section（`astra.vn.policy_actions.v1`，上限 4 MiB、64 个 command）；其源码 hash、注册 entry 和 descriptor 必须一致。文本 allowlist 使用作者层 `asset:/` ID，经 Cook 已有 `asset.catalog` 唯一映射到 `package:/`，缺失或重复映射、非 text 类型、错误 profile、UTF-8 或完整性失败均阻断。每篇文本最多 1 MiB、合计最多 64 篇，预载字节从现有 asset cache 预算扣除；未授权资产不预载。catalog/VFS 元数据各最多 16 MiB。Web 不支持 native worker：跨平台 Cook 可保留动作，但选择 Web 打包时拒绝非空动作绑定，浏览器打开动作包也会拒绝；普通无动作包不受影响。

项目配置与同一 `.astra` 的调用示例：

```yaml
nativevn:
  policy_actions:
    - entry: Policies/note.luau
      read_text: [asset:/text/note]
```

```astra
load_note resource:asset:/text/note #@id note.load
```

```lua
astra.command.register("load_note", {
    schema = "demo.note.v1",
    fields = { resource = { kind = "asset_uri", required = true } },
}, function(args)
    local text, status = astra.host.read_text("note", args.resource)
    if status == "pending" then return astra.action.await("note") end
    if status == "interrupted" then return astra.action.complete() end
    assert(status == "ready")
    astra.snapshot.set("note_length", #text)
    return astra.action.complete()
end)
```

Player 和 Headless 在既有 60 Hz 逻辑 pump 调用 `tick_policy`，结果按请求序号在 tick 边界接受；不依赖输入或呈现帧率。系统页保留剧情 fence 并暂停 handler，返回后继续；关闭和改走其他剧情时取消旧请求。只在剧情等待改变或确有显示输出时刷新画面。首条动作位于首句对话之前时，保存恢复不向尚未启动的 Yakui context 发送恢复通知。

### Timeline 段插值与源码事务

本批将已有四种 `VnPresentationEasing` 收归 script：`linear`、`ease_in`、`ease_out`、`ease_in_out`。`.astra` 的 `timeline easing:` 是整条轨道的段规则；省略为 linear，每两个关键帧之间独立按该规则计算归一化进度。沿用现有二次缓动公式，不把名称解释为 CSS cubic-bezier，也不增加通用曲线系统。首尾值精确钳定；取消沿用目标任务规则；保存保留轨道规则和已过时间，恢复不重启曲线。

Timeline 是同一源 AST 的临时投影。共享 `astra-vn-editor` 提供投影和版本绑定的编辑 batch；桌面与外部工具使用同一事务，不保存第二份时间线。修改仅替换属性 span，插入可选 easing 时插在命令关键字后，保留注释、source ID 与其余文本；一次提交对应一次撤销。Story、Scene、角色和 UI 继续使用现有 AST/IR，不新增未被运行层消费的 Sequence 模型。

compiled project 升至 v5、package root 升至 v4；包含轨道的 Stage/Player 存档同时换版本；VN 业务 RuntimeState 不含轨道，保持 v4。旧格式在解码前明确拒绝，作品重新 Cook，不做迁移或兼容别名。局部检查覆盖曲线端点/中点、编译诊断、暂停恢复、取消、文本与 Timeline 往返及版本冲突；桌面实际操作保持暂停。

模式参考：[rust-analyzer 的无损语法树与类型 AST 分层](https://rust-analyzer.github.io/book/contributing/architecture.html)；缓动使用已有实现，概念上的段进度映射参考 [CSS Easing](https://www.w3.org/TR/css-easing-1/)，不宣称 CSS 数值兼容。

### Linux WAV 解码

Linux 显式 GStreamer provider 的 one-shot audio 接受 `wav`，保持原有 48 kHz、双声道 float PCM 输出；不切换到其他 decoder。运行环境须提供 playback/typefind、wavparse、audioconvert 与 audioresample 插件。缺插件或流错误返回 `ASTRA_LINUX_GSTREAMER_DECODE_FAILED`，不传播可能含私有临时路径的 GStreamer debug 字符串，不提交失败请求的 sequence。启动或解码失败后同样将 pipeline 置为 Null。此改动不更改 package/save schema，不需要数据迁移；发布验证必须实际解码目标位深素材，codec allowlist 或构建通过不能证明插件可用。

Linux 视频使用 `Start/Next` 契约：Start 保留管线并返回可选 `duration_estimate_us`，Next 返回连续编号的 BGRA 帧或唯一的末尾计数；视频 OneShot 明确拒绝。探测时长可能随 demux 更新，不用于裁剪帧、判断完整性或确定循环周期。解码 session 持有单 worker 与容量为 1 的请求队列，appsink 最多保留 2 帧；一个前视 sample 以真实 PTS 差计算可变帧率时长。末帧使用解码器提供的有效 duration，在纳秒域计算帧尾后转换为微秒，保留原 PTS，不要求它与容器估计相等。缺时长/PTS、非递增 PTS、损坏帧、demux/decode 错误及超限均失败，失败 session 必须关闭。成功 EOS 确认已交付视频帧结束；容器未报错的静默截断不能仅凭时长估计检测，不宣称通用文件完整性保证。

Player 复用 `VideoFrame` 和 `VideoStreamEnd`：成功 EOS 后，最后一帧的实际终点确定完成时长；完成通知与下一轮循环等待该时刻。后续轮的时间偏移使用已完成轮时长，并核对每轮实际终点一致；时间加法和乘法检查溢出。媒体 snapshot 使用 `astra.player.native_vn_media_snapshot.v4`，视频保存已展示 `frame_end_us` 与可选 `loop_duration_us`；首轮 EOS 前后者为 None。恢复重新打开已绑定资产并按 cursor 解码，核对实际帧尾与字节计数，不把可变探测时长当资产身份。旧 v3 直接拒绝，无迁移或字段别名。视频流只承载 BGRA 帧；`movie_audio` 由独立 AudioHost 的 PCM 时钟管理，视频结束与循环不关闭或截断该音频。

Linux 本地媒体边界限制单次编码输入 256 MiB、单个视频帧和 one-shot 音频输出各 64 MiB；累计视频预算继续由产品 MediaHost 校验，不缓存完整解码影片。通过 `gstreamer-video` 按 plane stride 读取 BGRA，剔除行尾 padding。耗时解码不占用窗口事件线程；请求接收端取消或 session 关闭时 worker 停止拉帧，管线设为 Null 后删除临时输入。显式 CloseDecode 等待 worker，并传播关闭失败；取消的 OpenDecode 回复回收刚分配的句柄。解码资源继续使用既有 OpenDecode/CloseDecode 生命周期，不向公共边界暴露 GStreamer handle。

实现只增加 Linux target 的 `gstreamer-video = 0.24.2`（MIT OR Apache-2.0）与锁定的 sys 依赖（MIT），不传递第三方类型到公共契约。平台发布检查必须验证所选 GStreamer 插件、完整视频帧/PTS/结束计数、取消及关闭，而非只验证 codec 名称或首帧；真实窗口、设备音频与新集成分支另行验收。


Web 输入观察采用 `astra.player_web_live_evidence.v2`，Player 输入 transcript 采用 v3：静态 session id、递增 fixed step、真实输入消费与 typed route/terminal/choice 是验收来源。删除动态 provider id 及每帧三种摘要字段；观察边界计算的 `trace_hash` 必须与对应消费记录一致。拒绝旧 schema、缺失或跨 session 身份及缺失 terminal 状态，不构造兼容 hash，不把编译检查当浏览器实玩。

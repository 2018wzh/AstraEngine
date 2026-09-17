# 全产品重构实施状态

Musica v4 导出插件与 Manager 已构建，动态 ABI 布局和日志进入 Manager 的两项测试通过。GPU 点阵转场首次真实重跑在平台入口校验处被拒绝；现已补齐平台的遮罩命令和栈校验，定向测试与 Clippy 通过，真实路线仍需继续重跑。

按用户要求，旧 Minori family 统一重命名为 Musica：crate、Rust 类型、诊断与配置命名空间、CLI、工具和文档路径同步更新，不保留旧入口或别名。来源分支名称 `codex/minori-runtime-followup` 保持不变；游戏素材和已有私有存档未修改。重命名后的 76 项核心测试、12 项 CLI 测试、三个 Python 工具入口检查及受影响 Clippy 已通过。

日志桥已按用户要求移除脱敏和字符串白名单，保留大小限制、非法字段计数及 Manager 统一 sink；API/ABI v4 要求插件同步重建。Musica 来源分支 00610272d 与当前未提交修改正在重新核对，已确认完整演出和系统页存在遗漏，后续按功能移植并复用 SDK，不把旧 Host 接口带回主路径。

点阵转场已改用共享 GPU 像素遮罩，入场场景只生成一次，命令数量不再随视口像素数增长。新增硬件 GPU 测试通过逐像素图案、嵌套交集、栈恢复和错误后重新提交检查；1440p 八层场景分配低于 128 MiB。Classic 005 已用修复后的 Release 构建完成 GPU Headless 输入流程：48,096 条输入、39 次选择到达 tsui.ending，DX12 独显，运行无诊断，256 MiB 预算保持不变。终点 PNG 已查看，为黑场；这不代表全程视听、真实 Player 或 37 路线验收完成。

Musica 长流程在 tick 4141、命令 55 的 `.effect *` 返回 `ASTRA_EMU_MUSICA_RUNTIME_EFFECT`；现已从来源分支移植该命令的清除语义，76 项 Musica 测试通过（4 项显式 GPU 测试未在此命令运行），真实长流程仍需重跑。GPU 初始化日志已通过 Manager 显示 DX12 与 discrete_gpu。GPU 失败日志保留操作名和错误类别，日志桥现按用户要求取消脱敏。Classic 005 先前的点阵转场顶点放大问题已由共享 GPU 遮罩修复并完成路线复测。

Musica 新插件的真实素材 GPU Headless 已越过双资源 stage 阻塞，生成第 120、240、600 帧；已查看第 600 帧的立绘、背景和日文文字。随后遇到文字渲染失败，公开标点测试复现了手写覆盖范围遗漏省略号等字符的问题。现改从字体实际字符映射生成覆盖范围，astra-text 的 17 项测试、Musica 的 76 项普通测试及显式 GPU 文字回归通过，相关 Clippy、格式与文档检查通过。真实素材同输入序列复测完成 3600 帧并正常关闭，已查看最终场景和含省略号的日文文字；NullAudioDevice 消费 2156640 帧，记录 3912658 个非零样本。这验证了该段剧情与测试音频队列，不代表真实可听音频、完整路线或结局通过。

共享 GPU renderer 已支持命令默认颜色空间，Musica 场景选择 EncodedSrgb。Screen 半透明纹理在两种空间得到各自预期像素，透明像素保留背景；公共平台 crate 的 36 项测试通过，包含 7 项显式执行的硬件 GPU 测试。Musica 场景及立绘的 5 项回归通过，覆盖 GPU 绘制、裁剪、恢复和失败帧保护。双资源静态 stage 已接入，按背景、立绘、主前景、前缀顺序绘制；修复同一纹理重复出现时绘制 ID 冲突。场景回归增至 7 项，包含 3 项显式 GPU 测试，覆盖 Screen 混合、固定前缀原点、恢复和失败帧保留。`_ov.png` 原生混合及 SQZ 动画尚未实现，不能据此宣称 Musica 完整剧情可运行。

集显 GPU 提交计数已修复：timestamp resolve/copy 批次实际调用了 queue.submit，但旧计数只包含场景、atlas 与滤镜提交。现在按实际回读批次累计，下一帧开始重置；多个 pending frame 共用一个批次只计一次，轮询和再次读取同批次不重复计数。原先 3/2 次提交断言保持不变并通过，新增两帧批量回读和重置回归。公共平台全部 36 项测试通过（包含 7 项硬件 GPU 测试），全目标 Clippy 与格式检查通过；这只关闭提交计数缺陷，不代表整体验收或性能目标达成。

公共 wgpu 后端已实现 Screen 混合，复用现有 BlendMode 和 atlas pipeline；Sprite/Glyph 的批次现保留声明的混合模式，不再固定为 Alpha。两项新硬件 GPU 测试验证编码颜色空间、线性光纹理、透明度及 opacity 的实际像素，受影响 Clippy 和格式检查通过。公共平台 35 项功能测试通过（含 6 项 GPU 测试）；此前集显性能测试暴露的队列提交漏计已在后续修复，原断言未放宽。Musica 原生 `_sc.png` 的 Screen 分派与查表公式已核对并写入脚本文档，完整双资源 stage 尚未接入。

当前原生 Windows Player 已完成 Release 构建，并以四条路线测试使用的同一 package 重新打包。直接在 Sandbox 共享目录提交 bundle 遇到文件占用；改在未共享的任务目录完成打包后复制，生成成功。Sandbox 已实际显示终之空标题页，点击开始后因 `audio.open / ProviderUnavailable` 退出；未进入剧情，音频设备与完整 Player 验收仍开放。FVP 在显式测试音频模式继续自动播放，尚未到结局。

Headless 启用 FFmpeg 后的构建阻塞已修复：原生解码器包含不能跨线程移动的状态，现改为在会话专属线程内创建、运行和释放 HostState，启动错误经异步握手返回。旧视频测试已迁到 typed 增量流接口，检查完整帧序列、PTS、像素变化、字节总量和关闭；旧 OneShot 视频请求明确拒绝。Release FFmpeg 构建、18 项宿主集成测试（含 4 项硬件 GPU 测试）及受影响全目标 Clippy 通过。当前终之空 IR 未生成 movie 播放命令，Director movie 章节名称不能证明视频播放；源素材与转换行为仍需核对，真实 Player 视频验收保持开放。

Classic route.coverage.004 已完成同一 Release build/package 的原生 VN GPU 输入流程，消费 47,815 条输入、38 次选择并到达 tsui.ending，运行校验通过且无诊断，使用 DX12 独显。终点 PNG 已查看，仍为黑场；该 profile 的视频解码仍关闭，因此只能确认路线推进与终止，不能关闭全程视听或真实 Player 验收。route.coverage.003 也已结束：47,771 条输入、36 次选择到达同一终点，校验通过；同样不能据此关闭全程视听验收。

Musica 默认原点的静态 PNG 立绘已按原生 `ol/ot/or/ob` 边界与高度裁剪接入公共 GPU 渲染。PNG reader 复用现有锁定版本，元数据缓存有界，纹理沿用 SDK，裁剪不复制或改写像素。73 项普通测试、Musica/CLI Clippy 通过，另显式通过 GPU 定位、裁剪、保存恢复、恢复完整高度和失败帧保留测试。双资源前景、非默认原点配置及 SQZ 动画仍开放；不能以此关闭实际第 10 条 stage 或结局验收。

Musica 原生 stage 调用链已继续核对：冒号分隔的前景资源进入两个独立缓冲，不能套用 CrossFade2 时间序列；立绘附加参数参与可见高度裁剪，并受资源边界和原点模式影响；PNG 加载还会查找同名 SQZ 动画。具体规则已补入脚本执行文档。绘制顺序、边界来源和动画时序仍待接入，当前真实 stage 阻塞未关闭。

FVP 在 Windows Sandbox GPU 长流程中完成新槽 007 的原生保存与读取，恢复相同场景和对白后，物理点击继续推进文字，随后恢复自动播放。已有槽位保留；本次使用显式 NullAudioDevice，不计入可听音频或结局验收。

Musica stage v8 真实 GPU 重跑返回 `ASTRA_EMU_MUSICA_STAGE_SEQUENCE`（退出码 1），确认旧参数解析错误已越过，双资源序列绘制仍未实现。此前一次启动退出码为 101 且未捕获错误，原因未确认。Manager headless 现通过现有 astra-observability 在输出图像旁写入有界诊断日志，并记录关闭结果，避免只依赖控制台输出。

Musica stage 已由丢失立绘参数的通用图层映射改为完整原生状态：保留一至两个前景资源、参考坐标、背景和 `position[,resource_parameter]`。解析和存档共用参数校验，VM schema 升到 v8 并拒绝旧格式；正常绘制与恢复共用 Scene。70 项 Musica、12 项 CLI 测试及 Clippy 通过，另单独执行了 GPU 定位/恢复一致性与失败帧保留测试。双资源序列和立绘附加参数目前仍明确拒绝绘制，尚未关闭《夏空的英仙座》第 10 条 stage 的实际播放阻塞；本次补齐状态保存，未以忽略参数的画面代替原生实现。

FVP 在 Windows Sandbox GPU 重测中确认容量失败根因：BGM 请求到来时已有 2 个占用声部，其中只有 1 个处于 Playing，触发 hosted 混音器默认的 2 声部上限。Family 现按 RFVP 0.6.0 原生 BgmPlayer 的 4 个槽位配置 BGM 容量；SE 和总上限不变，不提前结束淡出或移除暂停声部。33 项测试通过，4 项独立 GPU 测试未在本次单元测试中执行；新增回归覆盖淡出与播放交叠、暂停占用、第五声部拒绝和释放后重新播放。Clippy、插件构建通过，同一原生存档的修复版 GPU 重测已越过此前失败的咖啡店段并进入后续场景，Manager 未新增容量错误；长流程继续运行。使用显式 NullAudioDevice，尚不代表真实可听音频或结局验收。

Musica CLI 的音频 census 原先将 `-` 识别为停止符，与运行时已有的原生 `*` 规则不符。现按 `*` 统计停止请求，不再将其列为缺失资源；不存在的普通资源仍报告缺失。12 项 CLI 测试及 Clippy 通过，真实归档重扫已通过：811 条音频引用中 410 条为停止请求，其余 401 条精确命中（BGM 167、SE 234），缺失、歧义与格式错误均为 0。旧 census 的 410 项 candidate missing 为误报；本次统计不验证音频解码和播放。

RFVP 的只读 `voice_count(kind)` 让 Manager 分别记录容量占用和 Playing 数量，暂停与淡出阶段仍占用槽位。fork 保持上游 0.6.0 基线加单个本地适配提交 73d3b44，本次容量配置修复仅在 Family，gitlink 不变，未推送。

Musica 的第 10 条失败指令已确认为 `stage`：前景使用冒号分隔的资源序列，stand 的第二项使用逗号分隔参数；此前解析器把前者当作单一文件名、后者当作单个整数。现已吸收 Musica 的这两类解析，但来源 provider 未消费完整状态，仍不能直接以忽略附加参数的方式接入。失败前的第 240 帧已查看，物理确认键确实推进了对白。

此前 FVP 同存档重测只记录到 Playing BGM 为 1、SE 为 0，未包含暂停或淡出声部，因此当时未调整容量；后续新增占用诊断才确认上述根因。

Musica 真实 GPU 连续输入已定位为 tick 421、指令序号 10、字节位置 319 的六操作数指令失败；会话随后正常关闭。失败前指定帧截图成功写出，尚未到达后续截图帧。此处早期诊断白名单已由 API v4 无脱敏日志桥替代。实际指令语义与 FVP 容量根因仍在排查。

Classic route.coverage.001 与 .002 已完成原生 VN GPU Headless 的完整输入序列，分别消费 46,997 与 47,793 条输入并到达 tsui.ending；运行校验通过且无诊断，使用同一 Release build、package 和 DX12 独显。终点截图已查看，均为黑场；这只能确认当前转换包的路线推进与终止，不能证明片尾影片或全程视听正确，profile 的 video_decode 仍为 disabled。Classic .003 与 .004 已继续运行，Modern、37 路线整体和真实 Player 验收保持开放。

Musica 指令失败新增脚本摘要、指令序号、行号、字节位置和操作数数量诊断，不输出 opcode 原文或参数；FVP 音频失败补充通道类别、配置上限和仍播放的通道数量，区分播放状态与混音器内部 voice 占用。未调整容量或跳过失败操作。FVP 32 项、Musica 65 项测试、相关全目标 Clippy 和构建通过；5 项显式测试未执行。带新诊断的真实流程仍须重测。

真实连续流程仍有两项阻塞：FVP 在 Sandbox 冷启动读取最新自动存档后再次终止，新增诊断将容量错误定位到 `audio play`；尚未确认是通道配置、重叠播放还是释放时机导致。Musica 的 3600 帧物理按键推进在完成前返回 `ASTRA_EMU_MUSICA_RUNTIME_OPERAND`，须继续定位具体指令及参数约束；不能把此前初始显示通过当作连续剧情通过。

Manager 的 GPU Headless 调试支持同一会话内按物理输入帧截图，用于比较真实游戏推进和存读档画面。请求数量、重复帧与范围在打开插件前检查；提前结束未到达所需帧会失败，图片写入失败保留关闭与 PCM 取消路径。指定像素写出、缺帧、请求边界及既有输入顺序共 3 项增量测试通过；真实存读档截图比较仍待运行。

Musica 共享图集修复后的真实游戏 GPU 重测已正常完成 120 帧并退出，输出画面经查看确认背景、说话人和正文可见，未再触发纹理上传越界。该测试使用既有脚本入口与显式测试音频；只确认此入口的启动和初始显示，不表示新游戏全流程、真实声音或结局通过。已继续启动 3600 帧物理按键推进测试，结果待确认。

Musica 真实 GPU 启动触发共享图集的宽度越界：增量分配器换行后仅检查高度，错误接受比当前 1024 宽图集更宽的资源，最终在 wgpu 上传时 panic。新增回归先复现，再补齐换行后的宽度检查，让现有重排/扩容路径处理大纹理。13 项平台库测试、显式硬件 GPU 宽/高纹理连续上传回归、Clippy 和格式检查通过；受影响 Musica 65 项、CMVS 151 项普通测试通过。修复后的动态插件已重新启动同一真实入口，结果待确认。FVP 的容量失败是另一项未定位问题，不能由本修复关闭。

Musica 已准备独立的《夏空的英仙座》18 分卷测试副本，原游戏目录保持只读。新增 `list-garbro-titles`，与导入共用有界 NRBF graph，只列出 PAZ scheme 名称，不输出密钥或写文件；重复名称和损坏结构拒绝。实际数据库列出 19 项，以 `Natsuzora no Perseus` 成功生成新私有配置。CLI 11 项测试与 Clippy 通过，并修正文档中 `--profile` 相对游戏目录的用法。真实归档盘点已解析 89 个脚本、33695 条命令，未知 opcode 计数为零；这仅表示 parser 识别，不能表示 VM 全部实现。音频候选仍有未匹配项。已知入口的 GPU 启动继续运行，尚未确认首帧或结局。

FVP Sandbox 长流程在后续校园剧情后以 `ASTRA_EMU_FVP_RFVP_CAPACITY` 终止，Manager 显示失败。当前信息只含通用 family operation，尚不能区分音频容量、播放状态同步或其他 hosted 边界，不能归因为已确认的上游缺陷。自动存档和手动存档此前的局部结果保留，完整结局仍未完成；下一步补齐适配操作诊断并从独立副本复现，不放宽容量或跳过失败。

FVP 适配层已为音频加载、播放、混音、提交与参数操作保留具体错误上下文，并将命令失败的稳定事件、操作与诊断码接入 Manager；播放状态同步和视频结束也有独立上下文。容量失败回归确认 Manager 可识别 `audio play` 且不暴露资源内容。导出 feature 下 32 项测试、Clippy、格式及构建通过，4 项显式 GPU 用例未重跑。更新后的插件已复制到 Sandbox 并重启 Manager，真实容量问题仍待复现；没有增加容量或改动上游核心。

共享 Symphonia 流式音频已修复截断输入被当作正常结束的问题。截断 WAV 回归先复现有效前缀输出后错误地返回完成，修复后返回结构化截断诊断；完整 WAV 和公开 MP3 仍正常结束，MP3 流式 PCM 与完整解码逐样本一致。14 项解码测试通过，不将此局部修复记作产品音频验收完成。

Musica 与 CMVS 已移除 GPU 回读后的额外整帧复制。Musica 保留共享像素缓冲供 Family 借用，CMVS 使用公共 `OwnedPixelBuffer` 的共享缓冲转换；写时复制继续保护已保留的旧帧。14 项 media-core 测试、CMVS 148 项普通测试及显式硬件 GPU 合成测试、Musica 63 项测试通过，后者包含 GPU 选择、存读档和转场回归；Musica 的独立真实游戏测试仍未执行。本项不改变 Family ABI 或存档格式。

基线：28f89d88。用户于 2026-09-12 确认重构范围，当前状态按实际代码与运行结果维护；历史 Stage 状态不代表新架构完成。

| 阶段 | 状态 | 尚需完成 |
| --- | --- | --- |
| 0 规则与测试 | 进行中 | 新宪章/契约与轻量文档检查已落地；EMU 独立 workspace 和 xtask 已接通；541 处普通测试已迁移，强制 Headless 宏与旧状态矩阵已删除 |
| 1 EMU 薄 API/FVP | 进行中 | typed 配置、可选翻译、驻留库与 FVP 编码适配已整合；待整合验证和真实游戏运行 |
| 2 SDK/Musica | 未完成 | 独立 astra-text、Musica archive/profile、Family session、实际视听与自有存档已整合；完整 opcode、动画/长媒体和真实游戏验收待完成 |
| 3 跨平台 EMU | 未完成 | 三桌面/Android Manager、核心与真实媒体运行 |
| 4 Engine/VN | 未完成 | 演出 tick 去除整会话克隆并修复排队存档；无包 World 与 typed Actor/Component 存档已接通，Runtime 整帧回滚、通用 replay 与历史 hash chain 已删除；其余任务、可信 Luau、typed 产品主路径和 DSL 待完成 |
| 5 Editor/Agent | 未完成 | GPUI、文本/图/时间线、独立预览、ACP/MCP 与两种编辑模式 |
| 6 终之空 | 未完成 | 新 .astra 工程、Classic/Modern 37 路线及私有四平台包 |
| 7 整体验收 | 未完成 | 全活动产品检查、真实流程、固定场景性能与旧路径清理 |

原生 Cargo 构建身份工具已移除对旧 Web UI 工具链预检的隐式依赖，不再要求 Node、jco 和外部 Luau analyzer 才能记录 Headless 构建。源码、工作树状态、Cargo 清单、依赖锁和 Rust 工具链及 feature 身份仍保留；未生成或伪造旧预检通过记录。新增原生构建回归覆盖存在旧 UI lock、缺少预检文件时的正常生成及身份摘要完整性。

Classic 路线驱动已增加输入预算预检：生成序列超过 profile 的消息数或 tick 上限时，在创建运行目录和启动 GPU 前拒绝，显示所需值与配置值；源 profile 保持不变。9 项 Classic Python 测试通过，包含精确边界、无效配置、拒绝时不启动进程或生成产物。运行期帧数预算仍独立校验。

终之空 Classic 第一条完整路线的 GPU 运行在输入 19327、有效 tick 106397 处触发 `artifact.scene` 累计提交帧上限，未完成路线。该私有测试 profile 的上限为 100000 帧，小于一次长路线所需；已改用四小时模拟时长预算的 Release profile 重跑，提交/光栅化上限 864000 帧，音频按同一时长计算。实际输入、剧情和终局断言保持不变，总产物字节上限仍为 8 GiB。第二条路线仍运行中，37 路线验收保持开放。

CMVS 接入审查已移除未解析脚本名的哨兵返回；缺失、空、多片段、循环或超深引用现在明确失败，避免后续适配层沿用旧 provider 的特定作品跳转硬编码。新增回归验证正常嵌套名称、失败时保留原 frame 和拒绝继续执行。148 项普通测试及 1 项显式硬件 GPU 层级/纹理尺寸替换测试通过，全目标 Clippy 通过。原生菜单脚本名写入和完整 Family session 尚未接入。

FVP Sandbox 自动播放进入后续剧情后因 `RFVP rejected save copy` 异常结束，Manager 正确显示失败。Family 文件适配层复制后以只读句柄调用同步，Windows 路径的成功复制回归已复现同类 IO 失败。现改为可写句柄同步并在原子替换前释放句柄，新增复制阶段、错误类别和系统码诊断；新建/覆盖复制与原有目标保护回归通过。FVP 30 项普通测试、导出 feature 全目标 Clippy 和格式检查通过，4 项显式 GPU 测试本次未执行。更新插件并重启 Manager 后，Sandbox 原生 GPU 系统页已完成测试槽 002 到空槽 004 的复制，源槽仍保留；读取副本、退出系统页后恢复原剧情，物理点击可继续下一段，未出现复制错误。音频仍为显式测试后端，不计实际声音验收。自动播放已重新开始，自动存档路径和完整结局仍待验证。

CMVS 指令执行已移除整 VM 克隆与隐式回滚，直接保留现有缓冲。执行错误或 panic 后失败标志阻止后续指令和快照校验；正常等待与帧停止的前置检查不使 VM 失效。恢复须使用单独校验过的成功状态。命令执行文件按职责拆成 14 个私有模块，入口保留统一栈读取、弹出和日志；155 个原有处理分支逐项比较一致，最大子模块 457 行。145 项库测试通过，包含连续 1024 条指令复用同一 64 KiB 缓冲、部分修改后失败、恢复与等待边界；1 项显式 GPU 测试本次未运行。全目标 Clippy 和格式检查通过。此变更未完成 CMVS Family session 接入。

SDK 音频解码改为同步借用源切片，直接复用 Symphonia 支持借用数据的 MediaSourceStream。Musica 的资产读取保留 AstraEngine OwnedByteBuffer，脚本、图片和音频消费者不再先复制整段归档数据；CMVS 普通音频与 MGV 内嵌 Ogg 同样删除解码前的整段复制。返回 PCM 独立持有采样，帧数预算、取消和格式错误语义不变。SDK 单元测试、CMVS 与 Musica 库测试通过，包含已有 GPU 会话、音频关闭与恢复回归；受影响三个 crate 的全目标 Clippy 和格式检查通过。尚未完成 CMVS Family session 与真实游戏验收。

FVP Sandbox 后续剧情已完成中途存读档往返：自动播放经过场景与人物切换后，在空槽 005 保存；原生存档页显示新缩略图和时间。返回剧情并推进下一段后，读取 005 直接恢复保存时的背景与文字，随后可继续自动播放。本次未复现早期槽位读取后停留在系统页的现象，但未与上游独立程序对照，不能据此认定相关行为已修复。测试仍使用 GPU 和显式 NullAudioDevice；完整结局、实际声音和损坏存档验收继续开放。

FVP 后续 GPU 设置页检查完成系统/演出、音量/文字、角色音量三个页面的切换，选择角色可更新对应文字颜色预览。通过原生控件将独白与对白自动播放速度从 7 调到 10，切页后值仍保留；退出设置与菜单后自动播放继续进入后续双角色场景。未验证这些设置的跨进程持久化，音量页显示不计实际声音验收。

FVP 原生自动存档页已显示本轮持续播放生成的多个场景记录。读取最新记录后，存档页关闭，出现与缩略图对应的钟楼场景过渡，随后继续进入教室剧情；再次打开自动存档页读取同一记录也能继续播放，未出现此前的复制错误。本次确认自动存档生成、读取和继续运行，未逐项核对恢复后的全部变量，也未验证自动存档跨进程恢复。仍使用 Sandbox GPU 与显式无声测试后端，完整结局和实际声音验收保持开放。

CMVS/Musica 共享能力改动整合后，独立 Emulator workspace 的默认 feature 构建、全部普通测试、全目标 Clippy（`-D warnings`）和所有活动成员格式检查通过。Musica 原生 GPU 场景、选择、恢复及取消阻塞音频写入回归通过；显式忽略的 GPU、动态插件、商业游戏与联网用例不计入本次通过范围。Siglus 上游编译警告仍保留，未为消除警告扩大 fork 修改。此次检查不代表根 Engine/Editor workspace 或全部平台已通过。

CMVS 配置改为 `astra.emu.cmvs.profile.v2`，归档从按 role 排序的映射改为有序列表，挂载和同名脚本查找遵循配置顺序。新增回归覆盖非字母顺序的归档优先级、重复 role、旧 schema 和旧对象格式拒绝，继续复用 SDK 文件路径校验。150 项普通测试、全目标 Clippy 与格式检查通过；既有显式 GPU 用例本次未重跑。实际 Family session、文字资源来源跟踪和媒体接入仍未完成。

CMVS 解码缓存、范围读取和流复用 Engine `OwnedByteBuffer`，缓存命中不再复制完整条目。脚本加载也直接解析共享缓冲，删除经流读取整份输入的第二次分配，保留 64 MiB 限制和成功解析后才安装 VM 的顺序。新回归直接检查共享分配，并验证缓存/归档释放后既有范围与流仍有效，源文件变化仍拒绝新读取。151 项普通测试、全目标 Clippy、格式和文档检查通过；本次不新增真实 GPU 或游戏完成记录。

Musica 音频恢复现按已解码音频的帧数与采样率校验游标上界，拒绝超过资源长度的存档，失败前不替换当前混音器。回归先复现越界游标被接受，再验证播放中与停止状态均被拒绝，原声音仍可继续播放；恰好位于音频末尾的停止状态仍可恢复。此项补齐存档恢复边界，不代表真实游戏长流程已完成。

Classic 批量输入生成器复用单路线驱动，保留物理输入、等待时序和终局断言，仅转换矩阵的 session/checkpoint 名称。当前私有 IR 的 37 条完整路线均已生成并通过既有矩阵输入检查，最大需求为 48,096 条输入与 343,278,530 个预算 tick。生成器逐路线处理，全部成功才提供最终目录；失败清理自身暂存文件，已有输出不覆盖。12 项 Classic 工具测试与 2 项矩阵测试通过。此次完成的是批量执行准备，尚不计任何新增路线验收通过。

矩阵执行器已删除旧版运行报告读取，启动时显式传入 GPU 参数，并复用单路线的硬件 adapter、构建、包、检查点和 manifest 校验。全部路线先检查输入预算，再创建运行目录；旧的汇总文件续跑入口已移除。新增模拟子进程测试覆盖 GPU 参数与软件 adapter 拒绝，预算回归确认失败不启动进程。用真实 Classic 输入和首条路线的较小 profile 执行预检时，第二条路线因 47,793 条输入超过 46,997 配额明确失败，未创建运行目录；未启动第三个 GPU 流程。已有两条完整路线仍独立运行，矩阵真实全流程尚未完成。

FVP、Musica、Siglus 已改用 API 内可选 ProviderModule，共用单会话 ABI 管理；panic 后仍可关闭，模块释放会关闭遗留会话。API 16 项单元测试、FVP/Musica/Siglus 与 Manager 的增量测试、受影响 crate 全目标 Clippy 和格式检查已通过。FVP/Musica 显式开启 dynamic-plugin-export 后，三个实际插件均通过 Manager 的 ABI 布局与诊断接入检查（共 6 项）；导出 feature 的全目标 Clippy 和 FVP 在 1 MiB 栈上连续三次开关会话的回归也通过。核心 VM、媒体和原生存档实现未因 ABI 复用而改写。

Headless 长流程新增逐输入开始/完成 TRACE，使用序号、输入种类和有效 tick 定位耗时，不输出观察值或商业内容。22 项单元测试通过，2 项显式性能测试未执行；逐输入日志现已用于修复版终之空完整路线运行。

终之空完整路线运行的主线程采样落在 GPU 场景 ID 的线性去重检查。Renderer 现对不超过 128 个 ID 的场景保留栈内集合，较大场景转为哈希成员检查，避免平方级查找；命令顺序仍由原始绘制序列决定。12 项平台公共层单元测试通过，包含 16384 个 ID 跨阈值插入与重复检查；新增硬件 GPU 回归通过 512 项绘制顺序、重复拒绝和拒绝后的正常绘制。这项局部回归不能单独证明完整路线的停滞已解决。

修复版 Headless 已启动同一终之空完整路线，逐输入日志确认越过第 124 条等待并继续推进；完整终局仍未完成。Windows Sandbox 中已用重新构建的 Slint Manager 安装 FVP 插件，从独立游戏副本显示原生启动与标题画面，点击进入系统设置页，并关闭会话返回 Manager。首次系统音频启动因设备缺失明确失败，Manager 显示 ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE；随后显式选择 NullAudioDevice，游戏页持续显示无声测试标记。此次只确认窗口、画面、输入、诊断和会话关闭，不计作真实声音、存读档、冷启动或结局验收。源目录仅只读共享，测试写入留在 Sandbox 副本。

FVP Sandbox 后续验证确认 Manager 冷启动可恢复游戏库与插件，测试音频选项重置为系统默认；显式重新选择测试后端后可打开原生标题。同一 RDP 连接内关闭重开正常，但断线重连后，同一 Manager 进程再次创建设备会失败。适配层现保留 wgpu 原生设备请求的具体错误，Manager 显示 Connection to device was lost during initialization；未切换 CPU 或静默重试。设备丢失的底层原因仍开放。本机新增三次 GPU 创建/释放回归通过，FVP 30 项普通测试、导出 feature 的 Clippy 与格式检查通过；其余 3 项显式 GPU 用例本次未重跑。

FVP 已在 Sandbox 的独立游戏副本完成一次原生存读档往返：新游戏进入剧情，在空槽位保存并显示缩略图与时间，推进文字后读取本轮存档，恢复保存时的文字和画面；退出系统页后可再次推进到下一句。随后关闭游戏会话并退出 Manager，重新启动同一构建，从原生标题的 Continue 读取前一进程创建的槽位，恢复相同剧情位置，点击后正常显示下一句。读取时曾短暂显示保存时的系统页；尚未与上游独立程序比较此行为。两次测试均使用 GPU 与显式 NullAudioDevice，RDP 连接保持不变；实际声音、损坏存档与完整结局仍未验收，原有源存档未改写。终之空完整 Classic 路线继续运行，已越过第 4573 条物理输入；优化版 Release Headless 构建完成，尚未用该构建完成路线或性能验收。

## 验收安排

Artemis 接续检查确认独立 Emulator 依赖图不含 mlua，因此旧分支为合并 VN 而改用 Luau、移除 send 的适配不再需要；优先保持上游桌面 Lua 5.1。完整历史候选现为上游 0c06f37 加单个本地适配提交 13e55ff，包含 PFS 分隔符修复、独立构建依赖固定与硬件 Vulkan 选择。主仓已登记精确 submodule gitlink 并补齐 Family 修改和许可证说明；Family 尚未接入活动 workspace，提交尚未推送。独立构建曾在 Cargo 清单解析阶段遇到可选 art3m1s-rfvp 对相邻 RFVP fork 的路径依赖。候选清单现将此可选依赖固定到 Alphaly2K/rfvp 的 ec204312e123b4839cec8e69e6237fb0374e5518，该版本提供 external-renderer 与 host-runtime；依赖解析已通过；首次 Vulkan 构建在 shaderc 原生编译阶段因 Windows 对象文件路径过长失败，缩短当前工作树内的产物路径后，Vulkan 配置 cargo check 已通过；显式原生 Vulkan 测试已通过三次创建、帧读回和关闭重开。Clippy 执行成功，上游警告保留；这不等于 Manager、真实游戏或视频音频验收。原生 Lua 5.1 配置的 asb-interpreter 227 项单元测试通过，未移植旧分支的 Luau/send 改动。PFS 9 项测试已通过，包括新增的大小写、混合分隔符与范围读取回归；原有路径转换测试改用原生 Path 比较，修正其 Unix 分隔符假设。此依赖不启用为 AstraEMU 的 FVP 运行路径，也不改变其既定基线。Artemis 已有 FFmpeg 视频 session，应复用该实现而非伪造视频完成。旧分支的延迟伪造视频完成和强制唤醒不可直接沿用。

Classic 物理输入驱动已支持指定生成路线执行到终局，不再只限 Y→K 段。路线追踪拒绝剩余未消费选择、不可用选项和目标终局不符；通过条件仍是实际 VN session 的终局观察。25 项相关 Python 回归通过。首条完整路线生成 46997 条输入，包含 39 次选择；私有测试 profile 按输入长度设置预算，未修改 Host 限制逻辑。完整路线尚未验收通过，37 路线完成数不变。

Native VN 本地整合回归通过：Player 保留 UI 操作产生的字形生命周期命令，文字显示完成状态通过只读观察提供给物理输入脚本；GPU Headless 在资源跨帧重用前提交待绘制场景。相关 Player、CLI、media-core、Headless 平台及 VN adapter 测试和 Clippy 通过，显式 GPU 资源回归通过，Player/Headless/CLI 构建完成。终之空生成器更新当前平台契约和演出替换策略，28 项相关 Python 测试通过。Windows CRT 打包复用 object 校验 x64 PE DLL，全部校验后复制；缺失、截断、错误架构和非 DLL 输入回归通过。此批不新增真实路线完成记录，37 路线与 Sandbox 长流程仍开放。

本地 Emulator 整合检查通过：活动 workspace 的默认测试、全 targets Clippy、构建及格式检查完成；文档检查通过。共享 GPU atlas 的跨帧纹理 ID 释放/重用回归和 astra-platform-common Clippy 通过。需商业素材、指定设备或真实服务的 ignored 测试仍保持各自验收范围，不计入完成。此次只固化 EMU 整合与共享 GPU 修复，VN 长流程和其余产品工作继续。

Siglus 的完整 fork 已合成为上游 e762f9f 基线加单个本地适配提交 c6c4f99，保留原提交历史，主仓 gitlink 已更新，尚未推送。确认并移除了 117 个文件的纯格式差异及 18 个 manifest 的全局警告屏蔽，实际适配涉及 13 个文件。Family 配置和阻塞 PCM 关闭测试、显式 GPU 离屏回读以及 Family Clippy 通过；上游编译警告保持可见，三项需要授权素材的真实游戏测试未执行。

RFVP 已由裁剪源码快照改为完整上游 fork 的 submodule。保留 0.6.0 基线 304e773387a9920c9db091ec1fd937c717aea949，单个本地适配提交为 4c67834；主仓 gitlink 固定精确提交，尚未推送。FVP 依赖改为上游 crates/rfvp 目录，恢复原生视频、bitmap、Anzu 等原有依赖和 feature 定义。hosted-gpu 编译、29 项常规测试、3 项显式 GPU 测试及 Clippy 通过；原生平台入口尚未逐平台构建，真实结局验收仍开放。

Musica VM 已按状态模型、演出、音频命令、选择和错误定义拆分模块，保持既有状态格式与执行顺序。拆分后的 62 项默认回归和新增诊断隐私回归通过，Clippy 通过；未实现命令的 Family 诊断保留指令序号，删除对错误显示字符串的诊断码解析，避免透传脚本内容。

Musica 的 select 已接通 Musica 解析、选择等待、键盘焦点、确认跳转与 GPU 文字显示。原生 Family GPU fixture 验证 F5/F9 恢复相同选择画面，VM 回归验证目标分支和损坏索引拒绝。新增 GPU 测试按进程单会话约束串行持有 provider；Musica 62 项默认测试通过，另显式执行独立文字 GPU 测试并通过，相关 Clippy 通过；此前 CLI 10 项测试通过。鼠标命中与 GPU 显示共用布局，回归覆盖悬停、无位置点击、行间空白、边界外点击及确认对应分支。原版界面一致性和真实路线仍待验证。

Musica 的 CrossFade 名称和空效果清除行为已接入 Musica 原有时间线与 GPU scene。3 项演出逻辑回归、1 项原生 Family GPU 帧回归及相关 Clippy 通过；清除后 GPU 帧不再包含旧效果，存档保持清除状态。真实游戏演出仍待验收。

Musica 的 playbgm2、playse4 与 deletevar 已合入 Musica，同一音频执行模块支持独立通道、停止和存档恢复。音频指令从 Runtime 大文件拆出，未新增平行核心。21 项 Runtime、14 项音频相关和 10 项 CLI 测试通过（集合有交叉），相关 Clippy 通过；实际新增通道设备播放仍未验收。

Musica 来源分支的带标签 chain 已合入 Musica parser、VM 与原生 session；目标标签验证成功后才替换脚本，存读档保留目标位置。Musica 52 项及 CLI 10 项测试通过，独立 GPU 文字测试按原条件未在本轮执行；相关 Clippy 通过。Musica 其余功能仍待整合，未修改来源工作树。

Musica 音频请求超时现在取消 sink、保留失败状态并拒绝后续命令，恢复替换前检查取消。13 项音频相关增量测试通过，包括实际超时后解除阻塞写入、关闭等待 worker，以及取消空恢复保留原混音器。Clippy 通过。

SDK audio feature 已替代 Musica 独立解码模块，CMVS 普通音频与 MGV 内嵌 Ogg 使用同一 Symphonia/Kira 浮点解码入口。相关 15 项增量测试与受影响 Clippy 通过，包括解码预算、取消、损坏输入、Musica 阻塞 PCM 退出和 session 存读档。CMVS 播放 worker、长媒体及真实音频设备未验收。

CMVS 已迁入 VM、CMVS 3.90 指令契约和核心自有归档访问，141 项测试通过；状态/单步调度分开，opcode 表拆成 8 个有界模块。全部 65536 个 opcode 输入与来源契约逐项比较一致，临时比较源码与测试程序已清理。旧调试输出改为 tracing 的稳定事件与受限字段；未导入旧 provider 的私有日志和整体 Debug 内容。脚本加载已直接连接核心归档，成功解析后统一更新 frame 身份、入口 PC、索引、字符串表和初始数据；非法 frame 或路径保持 VM 不变，关联 8 项增量测试通过；重载全零或空数据段会清除旧的稀疏字表，两项回归覆盖其他 frame 隔离与字节布局。脚本调用查找已连接同一归档加载入口，按挂载顺序匹配裸文件名；路径越界和未找到资源明确失败，诊断不附带私有名称。CmvsScene 已复用 SDK 纹理缓存和公共 wgpu renderer，硬件 GPU 回归验证层级合成与同槽纹理尺寸更换；完整 Family session、媒体连接与大型执行分派细分仍待完成。

2026-09-16，CMVS 的 CPZ、PB2/PB3/JBP、MGV、PS2A 与系统存档格式层从 `de4110e9e` 迁入，47 项既有格式测试通过。导入的大文件已按格式职责拆分，共用 SDK `CoreError`，删除 Musica 的重复错误类型。后续归档访问改为 `CmvsArchive`，typed profile 直接声明文件与 scheme，挂载前拒绝未支持的 CPZ 版本。来源 provider 的未提交改动尚未移植，真实 CPZ5 游戏流程未验收。

SDK 的归档数据类型、有界私有配置读取和可选明文缓存已由 Musica 与 CMVS 共同使用，Musica 原有重复模块已删除。SDK 无默认 feature 及 archive/cache/text/image 各自单独启用均可编译；不要求 Engine/VN session。配置读取失败不回写文件，错误不含原始输入和路径；相对游戏目录的回归覆盖配置路径仅解析一次。最近一次增量检查中 Musica 48 项、CLI 10 项和 SDK 7 项测试通过，独立 GPU 文字测试仍按原条件单独执行。

2026-09-16，`astra-emu-sdk` 已进入 Emulator workspace，提取 Musica 的文字资源管理和纹理缓存，复用 `astra-text`、`astra-media-core`、`image` 和 `lru`。Musica 删除 CPU 文字中间图层与 CPU 场景合成，直接使用公共 `WgpuOffscreenRenderer`；不创建 Engine/VN session。GPU 回归暴露并修复公共图集在跨帧重用临时纹理 ID 时跳过重新分配的问题。Musica 50 项测试（含 GPU session 的输入、存读档和关闭重开）、SDK 2 项缓存测试、独立 GPU 文字测试、公共图集 GPU 回归和 12 项单元测试通过；受影响 crate 的 Clippy 通过。此项是 SDK 首批实际调用方迁移，CMVS/Musica 整合、Musica 长流程与 Sandbox 验收仍未完成。

2026-09-16 从 `1748dd68` 建立本地独占分支 `codex/local-product-rebuild`，先验证 FVP《樱花萌放》和 AstraVN 终之空原生 Player。KrKr、Siglus、Artemis、CMVS、Musica 的本地已提交及未提交成果均纳入，来源工作树保持原状；Siglus 已开始导入，其余成果尚未整合。ACP 外部 Agent + MCP 方案保持不变。公共 Runtime 命名与 VN 专属职责分离，具体见重构契约。

用户于 2026-09-15 指定后续重构全部由主线程实施，不再使用子智能体；总体范围、阶段顺序和验收目标保持不变。

本环境优先做可执行的实现和自动测试；另一环境中的合法游戏源和设备于阶段验收时接入。Windows 承担终之空 37 路线和两款 EMU 各一结局；其他三平台执行代表流程。未接入设备的结果保持未验收。

性能目标为 VN 桌面 1440p120、移动 1080p60。记录实际设备与固定场景结果，EMU 按原生速率分别测核心与 Host。

用户补充设备范围为“主流配置”；CPU/GPU、内存、macOS 机型与 Android SoC 尚未指定，正式测量时记录实际型号，不能把“主流配置”当成已固定的性能基线。尽量通过远程环境完成工作；当前环境不可用的商业游戏与转换工程在另一环境可用，具体接入方式和设备远程权限待提供。

Agent 采用 ACP 外部进程与 MCP；不在 Editor 内置 OpenAI 模型循环。

## 当前验证记录

- 独占重构 worktree；并行子任务各用独立 worktree/target。
- 新文档检查 222 页通过；7 个链接/卫生回归测试通过。
- EMU 独立 workspace 全量 fmt/clippy/build/test 通过：169 项测试通过，3 项按 GPU/联网条件未执行；含 API v2、Manager、FVP 和 Musica CLI。
- xtask 与 Linux platform all-target clippy 通过；修复 default build 的 audio fault-injection feature 组合错误。
- 普通逻辑测试与独立文字库已整合：子任务验证包含 104 个普通测试、15 个独立文本测试和 5 个媒体适配测试。
- Engine 全量 fmt/clippy/build/test 复验通过：666 项测试通过、0 失败、9 项按原条件未执行。修复了显式 Headless fixture 生命周期、无效 timeline fixture 和日志队列时序测试；已删除旧矩阵检查。
- 演出改动子任务的 35 项测试、all-target clippy 和 Player VN 调用方编译通过；Musica archive/profile 子任务的 40 项测试与 clippy 通过。
- Musica session 整合后 core/CLI fmt/clippy/build/test 通过，45 + 10 项测试通过。公开 fixture 覆盖 PAZ/SC、实际画面、PCM、输入、存读档和取消/关闭；动态库构建由子任务验证。尚未支持的 opcode、ANI/SQZ session 播放、长媒体流式解码和 Android 注册保持未完成。
- Musica 音频渐变现进入 AMINSV02 自有存档，保存采样进度并按剩余时间恢复；实际 PCM 连续性、fade-out 停止边界、参数替换和未播放资源停止测试通过。最新 Musica/CLI 全 feature 50 + 10 项测试、clippy 和 fmt 通过；旧 AMINSV01 拒绝且不覆盖。
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

- NativeVN step 输入统一使用 Runtime TickInput/TickMode，移除输入/输出 session id 与中间 step_native；时间参数直接进入 TickRequest，旧 ABI 仅在边界查找会话和适配模式/身份。原生 close 返回 unit，旧 ABI 单独生成关闭 report。原有 provider/Player 单元测试及新增五类非法时间参数、失败后恢复推进回归通过。Engine 全量 fmt/clippy/build/test 通过：730 项通过、0 失败、9 项按原条件未执行；存档 v8 不变，package descriptor、配置与其他 ABI 消费者仍待清理。

- NativeVnRuntimeExecution 拆成独立模块，直接使用 Runtime TickIntegrityMode 和 usize worker_count，移除 ABI executor/mode 映射与 u8 范围转换；parallel 构造不再返回 Result。Player 与 Headless 调用方同步，直接依赖已有 astra-worker-budget。2 项回归验证进程预算一致性及 package 打开前拒绝 0/超限/usize::MAX worker 数。Engine 全量 fmt/clippy/build/test 通过：732 项通过、0 失败、9 项按原条件未执行；package descriptor 与其他 ABI 消费者仍待清理。

- PackageReader 对外改为 PackageRuntimeSelection（NativeVn/target/profile），删除插件选择对象 API；Player 与发布检查同步迁移。builder/reader 继续完整验证现有 policy/registry/target/VFS，并拒绝未知 runtime 描述。发布行为检查直接使用 NativeVnSession，验证容器恢复一致与恢复后 typed step，删除通用宿主、Tokio runtime 和剧情重复编码；清理直接依赖的默认动态 ABI feature。package 双入口拒绝与 NativeVN 发布回归通过；Player 打开包和发布入口继续严格匹配编译入产品的描述，新增能力漂移拒绝回归。Engine 全量 docs/fmt/clippy/build/test 通过：734 项通过、0 失败、9 项按原条件未执行；序列化 policy descriptor 和其他 ABI 消费者仍待改版移除，此检查不替代真实 Player 验收。

- NativeVnSession 改为长期持有 VnRuntime，移除每步完整状态克隆和 reducer 重建；Core 提供 deferred step 与匹配当前 wait 的绑定。执行失败后 step/save 阻断，成功恢复才解除失败；restore 在提交前构建并验证新 runtime，保存格式不变。3 项新增回归验证 4096 条 backlog 在连续 step 中保留分配、失败后作用域取消及 step/save 阻断与恢复、过期/空等待绑定拒绝；同时去掉默认启动和存储计数的全状态物化。Engine 全量 docs/fmt/clippy/build/test 通过：737 项通过、0 失败、9 项按原条件未执行。真实平台性能尚未验收。

- NativeVN 删除专用转发 FSM/action、控制锁和命令转字符串 PlayerInput 层；会话直接在 Runtime tick 后提交业务事件及 host await。共享 Runtime 新增 create_host_await，保留 scoped completion 与可靠存档。旧专用 FSM 存档明确拒绝且保持当前会话；新增无 FSM 的等待保存/恢复/完成回归，并扩展旧 FSM 预检拒绝与作用域保留测试。Engine 全量 docs/fmt/clippy/build/test 通过：738 项通过、0 失败、9 项按原条件未执行。真实平台及商业游戏仍未验收。

## 本地接续进展（2026-09-16）

- 终之空转换器更新为平台 profile v3，分离 Kira mixer 与 WASAPI output；生成器经 Rust 实际类型解析的回归测试通过。Classic 工程 Cook 与 package 完成，包含 1,093 个状态和 327 项资源。
- Classic 的 GPU Headless 启动与 Enter 进入剧情通过，使用 DX12 独立 GPU；尚未完成路线、系统页与存读档验收。Windows bundle 已支持显式提供匹配工具链的 VC CRT 并纳入文件清单，缺文件或无效文件头会拒绝打包，相关增量测试通过。
- 完整 Windows 包在 Sandbox 的原生 GPU 路径显示标题，点击开始游戏后在 `audio.open` 返回 `ProviderUnavailable`，当前环境没有可用音频输出。Player 已补充退出边界的错误码和操作名日志，避免只有窗口消失而没有文件诊断；未将此结果计作剧情、音频或路线通过。
- Classic 开场的 GPU Headless 物理输入检查已覆盖右键系统面板、Escape 返回、演出中保存、继续播放、读档及恢复后继续播放。保存前与读回后的 RGB 图像完全相同，恢复后推进到同一时间的画面也与连续播放一致；使用 DX12 独立 GPU。混音输出存在非静音 PCM，但不代表真实设备播放通过。当前仅覆盖开场片段，冷启动恢复、损坏存档、其他剧情及 37 路线仍未验收。
- Classic Y 段长流程发现旧包与当前研究 IR 的命令 ID 对应不同，原超时不能认定为 Runtime 故障。已固定私有 IR 快照并完成配套源码、输入和包的重建；转换器为六类舞台命令补全 `interrupt:replace_from_current`，真实 Rust 编译器回归测试通过。新包 GPU 长流程在输入 284、tick 11097 因未上传的字形引用失败，资源生命周期问题待修复，未计作路线通过。
- 字形故障排查增加资源哈希、原驻留状态、当前帧变更状态和命令位置诊断，不记录正文或路径。包含逐字显示初始化和连续物理按键的上传顺序检查通过，Native VN 宿主测试共 32 项通过；该用例尚未复现商业包故障，不能据此认定修复。受影响 crate 的 Clippy 通过，同包 GPU 长流程继续定位。
- GPU 重跑稳定复现输入 284 的错误：该字形从未上传，当前帧也没有对应上传命令。新增“布局缩放与剧情动作同批输入”回归复现了上传丢失；UI 资源 owner 已更新，但动作替换帧时丢弃了生命周期命令。现将这些命令保留到下一次实际提交，按顺序先上传再绘制；缓存 UI 只保留绘制命令，关闭也处理未提交的生命周期。失败回归已转为通过，44 项库测试、32 项宿主集成测试、Clippy、fmt 和文档检查通过。修复后的同包 GPU 长流程已越过原字形失败位置，随后在输入 285 等待剧情命令时超时；该等待问题继续排查，尚未计作路线通过。
- 桌面 Manager 删除隐式静态 FVP 注册，避免与显式安装的动态插件冲突；25 项 Manager 测试通过，1 项 GPU 条件测试未运行。
- FVP 现有全局存档的系统变量区与 HCB 不匹配，加载被拒绝且原文件保持不变。测试副本的新游戏与冷启动单帧通过；已有 CPU 软件渲染流程不计入 GPU 验收。
- FVP 已通过 `hosted-gpu` 复用 RFVP 原生渲染器，删除 Family 内独立 GPU 管线。动态插件启动 1800 帧到达标题页；新增原生遮罩溶解，28 项测试（含硬件 GPU 转场和 1 MiB 栈动态模块重复开关）通过。Sandbox 已进入系统页、切换文字设置、返回标题并进入剧情首屏。独立 RFVP `gpu-render` 检查通过。动态模块 session map 改为堆上持有，修复第二次插入的栈溢出；Sandbox 同一进程重开已通过。音频输出、存读档及结局验收仍开放。
- Slint 配置刷新保留未变更的 enum model，修复下拉列表被刷新关闭；回归与 Clippy 通过，Sandbox 已实际选择并保存配置。剧情乱码确认为启动编码选择问题，改为脚本对应的 Shift JIS 后，Sandbox 剧情首屏文字显示正常。Sandbox 没有可用系统音频设备，显式使用测试后端，不计入听感验收。
- 全部核心优先使用已有 GPU feature 和平台适配，仅修改必要嵌入边界；实际画面先用 GPU Headless 检查，再进入 Windows Sandbox，不以 CPU 渲染或 Host 上传 CPU 帧替代 GPU 验收。
- Siglus 已从固定宿主适配基线 `2be01aee` 导入独立 Emulator workspace，补充 Family API v2 动态入口和配置字段。保留核心 wgpu 离屏渲染及原生 PCM tap；移除游戏专用变量输出和错误链中的私有数据，启动失败清理宿主时钟，配置解析与首帧读回失败直接返回错误。核心仅在既有宿主边界增加 PCM 取消回调（先解除阻塞再 join），并拒绝 CPU/未知 GPU adapter。真实 Kira 阻塞写入连续三次关闭回归、配置回归、原生硬件 GPU 离屏读回、全部目标 Clippy 与动态库构建通过；实际插件加载、Sandbox 游戏流程和音频设备播放仍未验收。
- Manager 的真实动态加载器已分别加载 FVP、Siglus 构建产物，连续三次检查 ABI、描述一致性和空目录探测通过；动态库按现有契约驻留至进程退出。此项只验证插件加载边界，Siglus 的实际开局与 Sandbox 流程仍未验收。
- Siglus 的真实游戏测试改为显式 ignored，手动运行时缺少素材直接失败；不再通过提前返回将未执行的流程计作成功。全部目标测试中 2 项局部回归通过，3 项真实游戏测试及 1 项硬件 GPU 条件测试保持明确的未执行状态；硬件 GPU 读回已在此前单独运行通过。

- Sandbox 原生存档已写入并显示剧情缩略图。读档后却回到存档页，退出该页后的文字样式也与保存前不同；VM 恢复位置、输入残留与渲染缓存失效仍需定位，尚未通过存读档验收。

- 修复 RFVP 图像恢复丢失调色像素和显示尺寸的问题，沿用原生快照结构与 GPU 管线。30 项 FVP 测试全部通过，包含 GPU 原地恢复、新会话恢复和动态模块重复开关；实际游戏读档返回页面的问题尚未解决。Manager Headless 增加按帧投递物理键鼠输入，复用现有键码映射与 Family advance，非法键码、顺序和帧范围在启动插件前拒绝。

- 更新后的插件在 GPU Headless 通过键鼠输入进入实际剧情，并复现保存后恢复异常；Sandbox 新建 DATA002、推进剧情、读取该槽后仍返回保存页，退出后文字过亮。已定位保存的 VM 游标处于原生读档返回值检查位置，具体恢复缺陷尚待定位。FVP/Manager Clippy、构建、受影响包 fmt 及 224 页文档检查通过；这些局部检查不表示存读档已通过。

- 使用 RFVP 已有有界 opcode 诊断环确认实际读档执行了脚本重建分支，没有据此改动 VM 指令语义。新增原生系统字体的 GPU 描边恢复回归，验证重建文字 surface 后像素一致；31 项 FVP 测试通过。实际读档的页面与文字异常仍开放，下一步检查其他协程及脚本重建状态。临时私有诊断入口已从源码移除。

- Family API v3 新增进程级诊断 sink，FVP、Siglus、Musica 动态适配层共用可选 tracing/log 桥，Manager 统一接收。最小原生移植与诊断接入原则已写入宪章、重构契约、API 契约和开发手册。受影响 API/Manager core/三个核心共 117 项测试通过；另对 FVP、Siglus、Musica 实际动态库各连续加载三次，日志转发无重复。跨 worker 的 tracing/log、级别过滤及敏感字段脱敏检查通过，受影响 crate 和 Manager/Musica CLI 的全部目标 Clippy、fmt、构建与文档检查通过。Manager 的 FVP 原生 GPU Headless 运行完成 5100 帧，收到核心初始化、存档准备/写入/恢复和关闭日志，未出现非法诊断记录；使用 Null 音频，不计作实际声音或完整存读档视觉验收。

- FVP 的 Manager GPU 复测已收到 slot 1 的原生恢复事件，待处理线程请求为零；恢复后四次物理 Enter 能推进到后续演出，未出现完全失去响应。最终画面尚需与保存点及正常推进路径比较，音频与结局仍未验收。
- Classic Y 字形修复后的等待超时定位到输入脚本：对白进入等待时仍在逐字显示，第一次 Enter 只补全文字。真实 Native VN 宿主回归验证了该行为；产品观察 v3 增加只读 `text_reveal_complete`，Headless 与路线脚本先等待显示完成，再投递物理推进输入。同包 GPU 长流程待复测。

- 本地接续阶段复查：Emulator 活动 workspace 的 cargo test --locked --workspace --all-targets 、cargo clippy --locked --workspace --all-targets -- -D warnings、cargo fmt --all --check 和 cargo build --locked --workspace 通过；显式忽略的 GPU、授权游戏与外部服务测试不计入本次通过范围。Sandbox 中 FVP 新建槽位后推进剧情，再读取该槽位、退出原生系统页并继续输入，恢复到保存位置后可继续剧情，随后正常返回 Manager。仍需验证进程重开及完整结局；Null 音频不计实际声音验收。终之空文字揭示修正后的 GPU 长流程因测试环境结束而中断，无完成结果；已在确认旧进程不存在后重新启动。

- 重建 Sandbox 后，Manager 安装 FVP v3 插件并扫描游戏成功；系统音频启动返回 ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE，没有自动换后端。当前环境仍不能完成实际声音验收。

- FVP 新进程在系统音频失败后显式选择 Null 可重新启动，标题与原生 Continue 页面可操作。读取既有测试槽位后背景和花瓣恢复，Enter 可推进到下一句；刚读档时保存位置的文字未显示，冷读档视觉恢复仍未通过，需区分旧槽位内容与当前恢复路径。

- FVP 当前版本新建空槽位后关闭并重开 session，原生 Continue 能读到该槽位，但保存位置文字颜色明显变浅；问题不局限于旧槽位。点击 Save 首次推进一句、第二次进入系统页，输入处理顺序也需复查。GPU 文字恢复测试补充全新 MotionManager 与渲染器路径并通过，fmt 与文档检查通过；局部重建路径未复现真实游戏颜色差异，仍需沿实际存档与脚本恢复时序定位。

- FVP 原生存档捕获/恢复增加 DEBUG 数值文字状态日志，经 Manager 桥输出字体、RGBA、描边与揭示进度，不包含正文。5 项存档测试、受影响 Clippy 和文档检查通过。新本地构建的实际插件加载返回 ASTRA_EMU_FAMILY_LOAD_ABI，本次真实游戏数值采集尚未运行，需先定位加载差异；Sandbox 中先前构建仍可运行，不据此判定新构建可用。

- 上述 FVP 加载拒绝已定位为本次构建命令遗漏 dynamic-plugin-export，DLL 缺少 Family root symbol，并非已确认的布局不兼容。新增实际 DLL header/layout 测试可在插件初始化前直接定位此类构建问题；正在按既有手册重新构建。

- 渐变恢复缺陷已用纯原生状态测试复现：alpha 由 0 淡入 255，在 200/1000 ms 时存档得到 51；恢复清空 alpha motion，剩余 800 ms 不再执行。新增普通回归测试当前失败，尚未修复，不能沿用此前测试全通过结论。实际 DLL header/layout 与三次初始化日志测试已在正确 export feature 构建后通过。终之空重启后的 GPU 长流程已经失败，原因是单帧重复修改同一 texture resource，下一步需定位 UI/stage 生命周期合并路径。

- FVP RFVS v2 已保存并恢复原生 alpha、move、rotation、scale、z、V3D、sprite、snow、lip 容器；不再在读档时清空进行中的动画。遮罩渐变类型 4–6 恢复映射已修复。29 项常规测试和 3 项显式 GPU 测试通过，包含中途淡出恢复后逐步画面与不中断播放一致，以及 RFVS 编解码后的剩余时长恢复。旧 RFVS v1 缺动画状态，明确拒绝且不改写文件；RFVG 全局存档保持原样。真实游戏需新建槽位重测，尚未关闭视觉问题。

- FVP v2 实际 GPU Headless 完成 5100 帧，写入并读取新测试槽位 3，正常关闭。人工检查最终画面确认保存位置文字和紫色描边清晰显示，之前半透明停滞在本流程未再出现；原测试存档保留，音频为显式 Null，不计声音验收。Sandbox 新版重开及结局仍待验证。

- 终之空纹理重复修改错误补充结构化诊断：仅记录资源摘要、此前是否释放及当帧上传/释放计数。相关 atlas 10 项测试与 astra-platform-common Clippy 通过；未放宽资源生命周期校验，真实触发路径仍待定位。Sandbox 旧 Manager 已正常关闭，修复后的 FVP 插件副本已更新，尚未重开复测。

- FVP RFVS v2 已在 Windows Sandbox 完成新空槽位保存、退出游戏与 Manager、重启进程、原生 Continue 读取及继续剧情。关闭恢复出的原生系统页后，保存位置文字与紫色描边清晰，Enter 可推进至下一句；本次未复现半透明停滞。旧槽位未覆盖。仍使用显式 Null 音频，不计声音验收；完整结局及其他存档异常流程仍未完成。开发手册已说明 v2 动画恢复与旧 RFVS v1 的拒绝行为。

- GPU Headless 检查点模式已复现跨帧资源操作被错误合并：释放后再次上传同一纹理，在最终绘制时报单帧重复修改。现于资源再次变更前提交此前待绘制帧，不执行中间像素回读；新帧仍先验证，GPU 原有校验未放宽。新增 GPU 回归覆盖三次释放/重传、非法后续帧拒绝与最终绿色纹理；连同既有 4 项 GPU 测试、11 项普通 host contract 测试及受影响 Clippy 通过。终之空诊断长流程仍在使用修改前的二进制运行，修复版真实路线尚未复测。

- FVP 原生 motion 恢复在修改场景前拒绝越界或重复图像槽位及未知渐变类型，删除越界静默跳过路径。回归确认这三类错误保持当前场景不变；30 项常规测试、3 项显式 GPU 测试及受影响 Clippy 通过。其他存档容器与资源错误仍需继续覆盖，尚未关闭完整损坏存档验收。

- 加入上述校验后的 FVP 插件完成 3100 帧 GPU Headless 冷读档复测，进程正常退出；通过原生 Continue 读取既有 v2 测试槽位并推进，最终文字与描边清晰。Null 音频不计声音验收。

- 终之空诊断运行的纹理重复修改错误确认同一资源此前已释放（previous_release=true），与跨帧合并回归一致。修复版已使用同一 Classic 包完成 DX12 独立 GPU 复测：498 个物理输入事件、27 次选择走完 Y 段并到达 K 段边界，自动检查通过，关闭正常释放文字资源。最终检查点已查看，章节文字正常显示。此项仅完成 Classic Y 分段，不代表 Classic/Modern 37 路线、完整结局或 Sandbox 实际音频验收。

- 修复版首次启动误用 RUST_LOG，未启用附加推进日志；已停止该测试进程，并按 Headless 实际使用的 ASTRA_LOG 重新启动。该次主动停止不计测试失败或路线完成，重开运行仍待结果。

Musica 已从来源分支移植 `.panel 0` 关闭、`.panel 1 * filename` 自定义面板和 `.panel 3` 全屏面板。GPU 绘制保留模式 1 的原生底部偏移，模式 3 从原点绘制；无效模式、参数和路径明确拒绝。三项回归通过，覆盖替换、关闭、原生存档往返和硬件 GPU 像素结果。完整系统页与其他演出仍待整合。

Musica 控制 pragma 已从来源分支接入 Family 主路径，修复所有 pragma 被静默忽略的问题。Ctrl 快进要求 skip 与 control 开关同时允许，不自动确认选择，也不越过等待中的翻译。按键释放、双 Ctrl、失焦和挂起由 session 处理，脚本开关保存于 v9 状态，物理键不写入存档。未知 pragma 明确失败。专项 GPU 输入回归及核心/CLI 测试通过；已读跳读、自动模式和完整系统页尚未移植。

Musica 屏幕震动已从来源提交 00610272d 移植，包含 V/R 指令、原生方向表、动画时钟、替换及 transition/chain 清除。场景与面板复用 GPU 变换和裁剪，未增加 CPU 图像搬运。v10 原生状态保存余时、偏移和随机状态；非法振幅先检查范围，避免损坏存档触发整数取负溢出。专项测试已覆盖中途恢复、逐像素裁剪、清除和非法状态拒绝。真实游戏震动与完整演出验收仍开放。

Musica 震动补充 Family 主入口 GPU 回归：从归档脚本执行 stage/shakescreen/wait，经 F5/F9 输入保存和恢复，确认恢复后的像素与不中断演出一致，挂起停止时钟、恢复后继续。此测试覆盖 Host 调用路径，仍不替代商业游戏长流程验收。

Classic route.coverage.006 已使用修复后的 Release Headless 完成 DX12 独显运行，47,805 条输入、38 次选择到达 tsui.ending，运行无诊断。终点 PNG 已查看，为黑场；仅确认当前转换包的路线推进和终止，全程视听、真实 Player、Modern 与 37 路线总体验收仍开放。

Musica 已移植 hscroll/vscroll 与轴向 endscroll，背景坐标直接进入已有 GPU Scene。三项专项测试通过：正负方向与亚毫秒余时恢复、强制结束及新 stage 清除、非法存档不修改现场，以及 Family F5/F9 输入后的真实 GPU 像素和等待完成。v11 保存滚动状态并交叉校验背景坐标。线性 scroll、scrollxf 和 WScroll2 仍待整合。

FVP 在现有 Windows Sandbox 会话继续长流程：确认 Enter 与原生 AUTO 可从月亮场景推进至人物对话，文字清晰。已通过原生保存页写入此前为空的 009 槽，再切换读取页恢复；恢复后的文字、背景和立绘正常显示，001–008 未覆盖，AUTO 已恢复运行。当前仍为显式 Null 音频，不计实际声音或完整结局验收。

Classic route.coverage.007 完成 GPU Headless 输入流程，47,801 条输入、38 次选择到达 tsui.ending，DX12 独显且无运行诊断。终点 PNG 已查看，为黑场；完整视听与真实 Player 验收仍开放。

Musica 的二维 `.scroll` 已从来源实现接入，与轴向滚动共用 endscroll、stage/chain 清除、Family 时钟和 GPU Scene。v12 保存二维轨迹，并拒绝互斥滚动同时占用、轨迹与背景不一致或非法时长的状态。已增加斜向中途恢复、强制结束、损坏存档拒绝和 Family F5/F9 GPU 回归；scrollxf、WScroll2 和完整演出仍待整合。

Musica scrollxf 已从来源实现接入：按缓动变化裁剪窗口与偏移，使用共享 GPU clip/transform，保留场景/面板与文字分层。专项测试覆盖缓动、强制结束、中途恢复、跨脚本继续演出、stage 清除、空窗口与非法状态拒绝；v13 保存当前裁剪轨迹。WScroll2、Firefly、次级效果、人物动画和系统页仍待整合。

Musica 接入来源的无参数 `.effect end`，与主效果清除共用生命周期；额外参数明确失败，不清除正在运行的效果。专项测试覆盖清除后的时钟、存档与非法停止参数。WScroll2 的双层全景绘制和 sync 资源读取仍待整合。

Musica WScroll2 已从 00610272d 接入新 Family 时钟、v14 原生状态和共享 GPU 场景。专项测试通过，覆盖双层负向环绕像素、恢复、停止、非法替换及 sync 格式/数量边界。来源的 sync/period 保留为读取校验与状态，不宣称尚未实现的同步运动；真实游戏长流程仍开放。

Classic route.coverage.008 完成 GPU Headless 输入流程，47,829 条输入、38 次选择到达 tsui.ending，DX12 独显且无运行诊断。终点 PNG 已查看，为黑场；完整视听与真实 Player 验收仍开放。

WScroll2 的 Family GPU 回归通过：真实插件 session 经 F5/F9 保存与恢复，窗口挂起停止时钟，恢复后继续轨迹。FVP Sandbox 的 AUTO 长流程已推进到新的机舱场景，仍未到结局，当前为显式测试音频模式。后续按用户确认直接批量移植来源分支完整机制，保留整合回归，不再逐项重验原引擎语义。

按批量移植原则接入 00610272d 的 Firefly、Snow 与 SnowH：保留粒子初始化、曲线、速度、重生、独立效果槽及渐停机制，拆分为粒子调度、Firefly 数学和雪运动模块，复用 Family GPU Scene。原生保存/恢复与渐停回归，以及真实 Family F5/F9、暂停的 GPU 回归通过。整合时修正雪校验器的方向与初始边缘位置约束。人物动画、电影、消息/系统页等来源机制仍待继续移植，真实游戏长流程未完成。

人物槽与 char 命令从 00610272d 接入：共享 GPU 镜像、中心/底部锚点、透明度等待、stage 一次性保留及 v16 保存恢复。Family GPU 镜像与中途 F5/F9 恢复、stage 保留/释放回归通过。正文内延迟换图入口、电影和消息/系统页机制仍待批量整合。

Classic route.coverage.009 完成 GPU Headless 输入流程，47,809 条输入、38 次选择到达 tsui.ending，DX12 独显且无运行诊断。终点 PNG 已查看，为黑场；完整视听与真实 Player 验收仍开放。

消息解析、正文内延迟换图、自动推进标记与语音等待已接入 Family。正文换图的 GPU 中途 F5/F9 恢复及语音等待专项回归通过；语音时长复用 worker 已解码数据。新增 v17 延迟队列/等待状态，旧查询接收端在读档时丢弃。已补充 PCM 阻塞时关闭并断开查询接收端的回归。backlog、完整 Auto/已读设置、电影和系统页仍待整合。

ANI／SQZ 静态帧从来源解码入口接入 SDK 纹理缓存与公共 GPU 场景。GPU 专项测试通过 PAZ 读取、像素、缓存复用、保存恢复和失败帧保留；解码前的尺寸/内存约束测试通过。动画时钟与动态立绘尚未完成，不能据此关闭真实游戏长流程。

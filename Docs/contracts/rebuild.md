# 全产品重构契约

本契约记录用户确认的新边界，实施状态见 [实施计划](../status/implementation-plan.md)。旧契约只在尚未迁移的实现中描述现状，冲突时以本契约为准。

## Engine 与共享库

公共 crate 使用 `astra-runtime`，公共会话使用 `EngineSession`；公共场景、任务、时钟、输入和呈现不得归入名为 AstraVNRuntime 的层。`VnSession` 持有 VN 剧情和系统状态，`VnRuntime` 名称仅用于 VN 业务。迁移必须调整实际所有权及调用方，不增加旧名称兼容别名。

共享文字、图像、绘制、解码、混音与字节源可不创建 World/package/registry 而使用。Engine/VN 与 Minori 是实际消费者。NativeVN 以普通 Rust typed API 组合，FSM 不作为唯一修改入口；固定 60 Hz 逻辑和独立呈现共用明确生命周期。任务具有句柄、作用域、完成/取消/失败结果；旧代次结果拒绝。存档保存显式状态，删除通用回放和帧内全量事务。

## EMU Family API

共享音频的流式解码只将解码器明确返回的流结束视为完成。`SymphoniaAudioStreamDecoder` 遇到意外 EOF 返回 `ASTRA_AUDIO_STREAM_TRUNCATED_INPUT`；已输出的有效 PCM 前缀不代表整段播放成功，调用方须终止失败的播放请求。正常完整流和解码预算规则不变。

GPU 回读缓冲可以在进程内保留共享所有权。`OwnedPixelBuffer::from(Arc<[u8]>)` 复用原像素分配，克隆只延长生命周期，`make_mut_for_update` 继续按写时复制隔离修改；转换本身不校验图像尺寸，仍由 `TextureFrame` 的现有校验入口负责。Minori 直接持有回读缓冲，CMVS 将同一缓冲转为纹理；Family 帧借用有效期和 Host 同步复制约束不变，保存与 ABI 格式不变。

本地接续同时整合 KrKr、Siglus、Artemis、CMVS 和 Musica 的已有成果，全部使用新 Family API；Musica 的格式与执行能力合入 Minori。新增核心的完成标准为真实代表流程，包括启动、连续剧情、媒体、选择或系统页、存读档和退出重开。FVP 与 Minori 仍各验证一条结局。

FamilyDescriptor 增加有界 typed 配置 schema（bool/integer/number/string/enum，分组和默认值）；OpenRequest 增加相同 schema 对应的 typed 配置值。字段 ID 唯一，未知键、重复键、类型/范围/枚举不匹配都在 open 前返回可定位错误。未提供值使用声明默认值。配置只在启动时生效，Manager 按核心/游戏持久化，核心继续验证。

TextReplacement 表示可选能力，未传 service 表示本次禁用，不应阻止支持正文翻译的核心启动。PCM capability 仍要求对应 sink。CPU 帧借用和音频有界/可取消写入不变。ABI 变更递增 fingerprint，旧插件明确重装，不增加旧 reader。动态库加载后在进程中保留，session 关闭取消所有 worker/服务调用；切换游戏不卸载库。

FVP 共用 RFVP VM、原生媒体和存档，仅补必要接口。Minori 使用 SDK，自有存档不覆盖原版；原版兼容不属于当前发布要求。SDK 不能依赖 VN 或强迫核心创建 RuntimeWorld。

全部适配核心采用最小必要修改，优先启用已有 GPU feature 和原生平台适配。Family API 的 CPU 最终帧借用只约束跨 ABI 交付方式，不要求 CPU 渲染：核心使用自己的 GPU 管线，交付时回读最终帧。不得为接入统一 Manager 复制一套核心渲染器。Windows Sandbox 通过 GPU 虚拟化执行视听测试；软件 adapter 明确失败，不能替代 GPU 验收。

所有核心诊断接入 Manager，不能只写在动态库内部的独立日志系统。Family API v3 在 descriptor/probe/open 前安装进程级 DiagnosticSink；适配层可选用 API crate 的 diagnostic-bridge feature，将 tracing 与 log 统一转发。Manager 仍是日志 sink、过滤和 flush 的唯一所有者。初始化冲突明确失败，不保留无日志的启动路径。桥不改变核心 GPU、平台和存档实现，也不要求依赖旧 SDK。

跨 ABI 只传有界 typed 事件：级别、来源、稳定 event、数值/布尔及经审查的符号字段；不传任意 message、Debug、路径或商业正文。未结构化日志保留级别、来源、行号与脱敏计数，关键故障需在根因边界补稳定事件。此计数不是成功标记，不替代原有错误传播。

Rust Family 可选用 API crate 的 ProviderModule 复用 ABI 会话管理，FVP、Minori 和 Siglus 共用此实现。它直接适配现有 FamilyProvider/FamilySession，不引入引擎或 SDK 依赖，也不改变 Family ABI。一个模块只持有一个会话；打开、推进、帧借用和完整关闭共用互斥边界，关闭返回前不能重新打开。panic 后拒绝继续工作，但允许取回会话执行关闭；模块释放时也须关闭遗留会话。公共边界错误使用 ASTRA_EMU_FAMILY_SESSION_ACTIVE、SESSION、LOCK 和 PANIC 后缀，核心自身诊断保持原有代码，不保留旧 Family 私有边界错误别名。

### 外部核心与 SDK 的重构边界

吸收 `emu/krkr-hosted` 中外部核心 fork、最小适配提交和 submodule 的决策。来源提交 `dad452d2d` 将 RFVP、Siglus 改为 submodule，`bfc49f419` 引入 Artemis；这些提交用于确认来源，不整体移植旧 Family 接口。目标是保留完整上游历史，在固定上游基线上用一个适配提交承载必要差异，由主仓 gitlink 锁定精确提交。分支名只用于维护，不能替代提交固定。

Family descriptor、配置、日志桥和 ABI 转换放在主仓适配层。fork 只补嵌入入口、原生 GPU 输出、PCM 接口及必要生命周期能力，尽量保持上游平台入口可用。核心自身缺陷整理最小复现后交上游 issue，禁止附带商业素材、私有路径和存档；必要临时适配说明原因和删除条件。不以本次重构为由重写外部 VM、渲染器或媒体系统。

整理后的 fork 保留许可证、归属和上游基线，Family 的 `MODIFICATIONS.md` 记录每项差异。新增改动在独占本地分支合成单个适配提交并验证，再更新 submodule；不重写来源工作树或已发布历史。本地提交不等于已发布，推送仍在本轮授权范围之外。

Musica 的成果合入 Minori，和 CMVS 一起驱动 `astra-emu-sdk` 模块化。先对照 AstraEngine 现有文字、绘制、解码、混音与字节源实现，再提取两个核心共同需要的能力；格式解析、VM 和原生存档语义继续由核心拥有。SDK 按需组合，不依赖 VN、World/package/registry 或强制 EngineSession；不建立纯转发 provider，不为成熟外部核心增加 SDK 依赖。替换调用方后删除重复实现。

## 创作与编辑

.astra 是 Story/Scene/Sequence/角色预设/UI 的唯一创作来源。成熟 CST/AST 工具链保留注释/source map；布局保存作者元数据。高级逻辑是可信 Luau，保存显式状态，IO 通过平台宿主异步执行。

GPUI Editor 使用独立真实 GPU 预览窗口。seek 仅当前片段，重建不重复外部 IO。统一版本化编辑 API 服务 UI、ACP 和 MCP；自主/逐批确认模式都支持取消、冲突检查、批量撤销。ACP 的模型配置由外部 Agent 持有。

## 测试、迁移与交付

普通测试不依赖 Headless；视听测试按需启动宿主。错误、损坏数据、取消、保存恢复及资源释放均需测试。删除旧双轨时同步所有真实调用方，不删除仍有用的产品行为测试。

Root workspace 管共享/Engine/VN/Player/工具；Editor 与 Emulator 使用独立 workspace、lockfile 和产物。平台目标三桌面+Android，缺环境不声称通过。终之空本地转换私有包保留 Classic/Modern 37 路线，Windows 长流程、其余代表流程。旧内部 package/save 可重建，原商业存档必须保护。

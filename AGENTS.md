# AstraEngine 实施宪章

## 权威入口

- [总体架构](Docs/product/architecture.md)：产品分工与平台范围。
- [重构契约](Docs/contracts/rebuild.md)：目标接口、所有权和生命周期；[Family API](Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md) 记录尚未迁移的实现，不作为新架构的兼容要求。
- [实施计划](Docs/status/implementation-plan.md)：当前状态、执行顺序与验收；[性能审计](Docs/status/performance-upstream-audit-2026-10-12.md)：问题、证据和上游修复。

宪章只保留长期约束，提交号、测试计数和阶段进度写入对应记录。现行决定取代旧 Stage、证据审批、强制 Headless、Qt、通用 provider/动态 UI ABI 和 Luau sandbox 要求；切换时迁移消费者并删除旧链，不留兼容双轨。

## 实施与数据

- 主线程负责共享契约、任务拆分、整合与最终验收，可委派独立子任务。每个实施实例独占 worktree、branch、target 和服务；不得编辑、编译或清理其他实例工作树。共享文件指定唯一整合者，来源分支和既有未提交成果保持原状。实际使用的代理模型须可核实，不冒报指定模型。
- 共享契约先于实现修改。优先成熟 crate，按实际消费者与平台职责拆分；合并纯转发层，lib.rs 保持薄 facade，接近 400–600 行时按职责拆模块。Rust 类型是 schema 真源；跨平台脚本用 Python，命令示例用 bash/sh。
- 商业资源、正文、截图、存档、私有 profile/key 和转换产物只留 ignored 工作区。保留许可证和修改归属，原商业存档不得误覆盖；读取失败不回写原文件。
- 保留容器版本、长度/边界、唯一 section、内容完整性和明确错误。postcard struct 不使用破坏二进制读取的 `skip_serializing_if`。不以 fallback、静默跳过或假数据掩盖缺失能力。
- 日志复用 tracing/astra-observability，库不建独立文件 sink；私有路径、secret 和商业正文不进入可提交报告。
- 常规实现、检查和本地提交已授权。真实游戏发现的上游功能缺陷可独立提交 issue/PR，附非商业最小复现；远端推送、主线整合及发布范围以当前实施计划中的用户授权为准。提交用短祈使句，不改写来源或已发布历史；破坏性操作另行授权。

## Engine、VN 与 Editor

- Engine 是可独立嵌入的 2D 核心。Actor/Component、flat FSM、任务组合各司其职；World 不强制携带 package，业务修改不强制经 FSM action。
- 公共运行层为 `astra-runtime` / `EngineSession`；VN 入口为 `VnSession`，`VnRuntime` 仅表示 VN 业务。按职责迁移，不保留旧名称别名或内部产品动态 ABI 转发。
- 逻辑默认 60 Hz，呈现独立。保存显式状态；禁止每帧全量克隆/序列化，不建设通用 replay、跨平台逐位确定性或全程 hash 链。事务留在编辑、存档和配置边界。
- 任务具有 owner、scope、取消和完成结果；读档/退出使旧异步结果失效。新演出替换同属性旧任务，其他轨道继续。关闭即使遇错也须完成取消和 worker 回收。
- `.astra` 是剧情、场景、演出和 UI 的创作权威，复用成熟 lexer/parser/CST/AST；图和时间线回写同一源码，布局仅为作者元数据。
- Luau 是可信游戏脚本，文件/网络通过宿主异步执行；存档不保存闭包、协程栈或 native handle。预览 seek 不重复外部副作用。
- Editor 使用 GPUI，独立 GPU 预览复用真实 VN session；游戏 UI 使用 Yakui，EMU Manager 使用 Slint。Editor 使用 ACP 外部 Agent 与 MCP 版本化编辑、批量撤销、自主/逐批确认模式，不另写模型 API Agent 循环。

## EMU 最小原生适配

- EMU 独立于 RuntimeWorld、VN、product package 和 Engine registry。同进程单活动游戏，以统一 C ABI 嵌入原生核心；Dynamic/Static 共用协议，不保留旧 ABI 运行时兼容层。
- 核心拥有 VM、原生调度、资源、渲染、游戏 UI、音视频、输入语义和存档。Manager 拥有窗口、平台事件入口、显示滤镜及最终 Present；不强制公共 advance、Host Mixer、VFS 或 no_std Runtime。
- 核心交付包含游戏文字和 UI 的完整 CPU 或 GPU FrameSurface，至少支持一种实际可用路径。保持不可变租约、有界资源和代次；GPU 通过版本化扩展协商原生共享与双向同步，不跨 ABI 传 Rust 图形对象，不强制共享 Manager Device。
- 核心声明配置范围、默认值和实际生效尺寸；Manager 保存、展示并传入。逻辑舞台、资产像素、渲染尺寸、窗口输出与内容区域分离。窗口 resize 不暗改核心渲染配置，也不推进游戏逻辑。
- 外部核心保留完整上游 fork：通用原生修复与改进进入 fork/master，astra 相对它仅一个必要适配提交；主仓通过 submodule 固定精确 revision，MODIFICATIONS 记录基线与差异，不复制核心源码。
- 最大化复用原 VM、GPU、媒体、平台和存档能力，只改必要嵌入入口、输出、Hook 与生命周期。上游功能缺陷独立处理，适配回归在适配内修复；不以替换 renderer、重写执行层或回退功能缩小 diff。
- SDK 按实际消费者复用共享文字、绘制、媒体和字节源，不依赖 VN、不强制 EngineSession，成熟外部核心无需采用。具体核心的保存、逻辑尺寸与来源约束只在重构契约维护。
- 语义 Hook 由核心在安全点同步请求，Manager 固定扩展线程持有共享串行 Luau VM。等待不阻塞平台壳；错误暂停未完成操作，重试不重复副作用。取消、退出及错误界面走独立原生通道。
- 窗口与模态由 Host 执行并反馈实际结果，无窗口 Host 明确拒绝。动态库驻留至进程退出；会话关闭必须取消服务、停止生产并回收 worker，释放宿主回调前保证不再回调。读档、重开使旧上下文与迟到结果失效，不补跑等待时长。
- 动态库由适配层安装共享日志桥，静态核心复用宿主订阅器；初始化失败阻止加载。核心日志保留事件、级别、来源和有界 typed 字段，字符串/message/Debug/Display 正常转发，不做桥字段脱敏或白名单；超限计入 `dropped_fields`。

## 验收与记录

- 普通逻辑用 unit/integration test；视听路径使用实际硬件 GPU/音频宿主，允许 headless 或 GPU 虚拟化 Sandbox。软件 adapter、NullAudio、首帧和 synthetic fixture 各只证明其覆盖范围。
- 保留真实 shaping、字体 fallback、完整媒体、资源生命周期和错误诊断。产品完成必须来自实际操作，不用静态报告、窗口存在或测试数量代替。
- Windows 终之空保留 Classic/Modern 37 路线要求；EMU 的核心、结局和平台验收范围见当前实施计划。VN 桌面目标 1440p120、移动 1080p60；EMU 按原生节奏分别测核心和 Host，以同场景配置建立基线。明确记录上游阻塞与未覆盖项，不将跳过写成通过。
- 平台为 Windows/Linux/macOS/Android，Editor 仅三桌面；Web/iOS/RPG/TRPG/运行时 AI 延后。Android 默认将游戏导入应用管理目录，静态编译核心。
- 代码提交检查文档链接、fmt、受影响 crate Clippy 和相关测试；阶段末检查活动 workspace build/clippy/test 并运行对应产品。缺依赖、忽略项和失败如实记录；文档改动运行文档检查，不冒报产品验证。
- 文档以中文为主，保留 API/crate 名。只维护当前契约、结论和下一步；删除失效过程页，历史留 Git，不再叠加流水账、审批矩阵或报告 schema。

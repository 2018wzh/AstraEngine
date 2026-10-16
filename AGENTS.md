# AstraEngine 实施宪章

## 权威入口

- [总体架构](Docs/product/architecture.md)：产品分工与平台范围。
- [重构契约](Docs/contracts/rebuild.md)、[Family API](Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)：接口、所有权和生命周期。
- [实施计划](Docs/status/implementation-plan.md)：当前状态、执行顺序与验收；[性能审计](Docs/status/performance-upstream-audit-2026-10-12.md)：问题、证据和上游修复。

宪章只保留长期约束，提交号、测试计数和阶段进度写入对应记录。现行决定取代旧 Stage、证据审批、强制 Headless、Qt、通用 provider/动态 UI ABI 和 Luau sandbox 要求；切换时迁移消费者并删除旧链，不留兼容双轨。

## 实施与数据

- 主线程亲自实施，不启动或委派子智能体。每个实例独占 worktree、branch、target 和服务；不得编辑、编译或清理其他实例工作树。来源分支和既有未提交成果保持原状。
- 共享契约先于实现修改。优先成熟 crate，按实际消费者与平台职责拆分；合并纯转发层，lib.rs 保持薄 facade，接近 400–600 行时按职责拆模块。Rust 类型是 schema 真源；跨平台脚本用 Python，命令示例用 bash/sh。
- 商业资源、正文、截图、存档、私有 profile/key 和转换产物只留 ignored 工作区。保留许可证和修改归属，原商业存档不得误覆盖；读取失败不回写原文件。
- 保留容器版本、长度/边界、唯一 section、内容完整性和明确错误。postcard struct 不使用破坏二进制读取的 `skip_serializing_if`。不以 fallback、静默跳过或假数据掩盖缺失能力。
- 日志复用 tracing/astra-observability，库不建独立文件 sink；私有路径、secret 和商业正文不进入可提交报告。
- 常规实现、检查和本地提交已授权。真实游戏发现的上游功能缺陷可独立提交 issue/PR，附非商业最小复现；其他发布、外发和破坏性操作另按授权执行。提交用短祈使句，不改写来源或已发布历史。

## Engine、VN 与 Editor

- Engine 是可独立嵌入的 2D 核心。Actor/Component、flat FSM、任务组合各司其职；World 不强制携带 package，业务修改不强制经 FSM action。
- 公共运行层为 `astra-runtime` / `EngineSession`；VN 入口为 `VnSession`，`VnRuntime` 仅表示 VN 业务。按职责迁移，不保留旧名称别名或内部产品动态 ABI 转发。
- 逻辑默认 60 Hz，呈现独立。保存显式状态；禁止每帧全量克隆/序列化，不建设通用 replay、跨平台逐位确定性或全程 hash 链。事务留在编辑、存档和配置边界。
- 任务具有 owner、scope、取消和完成结果；读档/退出使旧异步结果失效。新演出替换同属性旧任务，其他轨道继续。关闭即使遇错也须完成取消和 worker 回收。
- `.astra` 是剧情、场景、演出和 UI 的创作权威，复用成熟 lexer/parser/CST/AST；图和时间线回写同一源码，布局仅为作者元数据。
- Luau 是可信游戏脚本，文件/网络通过宿主异步执行；存档不保存闭包、协程栈或 native handle。预览 seek 不重复外部副作用。
- Editor 使用 GPUI，独立 GPU 预览复用真实 VN session；游戏 UI 使用 Yakui，EMU Manager 使用 Slint。Editor 使用 ACP 外部 Agent 与 MCP 版本化编辑、批量撤销、自主/逐批确认模式，不另写模型 API Agent 循环。

## EMU 最小原生适配

- EMU 独立于 RuntimeWorld、VN、product package 和 Engine registry。Family 自持原 VM、文件、渲染、解码、混音和存档；Host 接收不可变 CPU frame lease 和有界 PCM。保持 CPU lease 边界，不跨 ABI 传 GPU/native handle。
- 外部核心用完整上游 fork，整合时基于上游 HEAD 加一个必要适配提交，由 `Emulator/ThirdParty` submodule 固定精确提交。`MODIFICATIONS.md` 记录基线与差异；不在主仓复制核心源码。
- 核心改动严格限必要嵌入钩子：入口、依赖注入、原生输出与生命周期通知。优先启用原 GPU/媒体/platform feature，不复制 VM、renderer、decoder 或存档实现，不为统一 Host 更换上游图形库。
- 引擎自身功能缺陷独立提交上游 issue/PR，等待合并后更新基线。适配引入的回归在适配内修正；不撤回必要修复后宣称验收，也不把功能修复长期混称为嵌入钩子。
- FVP 共用上游 V1/RFVS、`GlobalSaveDataV1`/RFVG；不增加旧存档识别、迁移、兼容 reader 或专门旧版本报错。
- Musica 接续以已确认的 `codex/minori-runtime-followup` 完整机制为依据，测试聚焦整合，不重复原语义逆向；统一使用 Musica 名称。Musica/CMVS 共同驱动可选 SDK，复用公共文字、绘制、媒体与字节源；SDK 不依赖 VN，不强制 EngineSession，成熟外部核心无需采用。
- Musica 短期用核心自有存档。GARbro scheme 保留纯 Rust 两阶段 NRBF reader：先收集对象、再解析引用；未知、断裂、重复、越界或非法对象图明确失败，不用 BinaryFormatter 或启发式 fallback。
- 分清逻辑舞台、资产物理像素、raster canvas 和窗口输出。复用公共 `Canvas2D` 等类型；资产显式提供逻辑尺寸和 ANI 原点，Host 负责 letterbox 与输入逆映射。Musica 逻辑舞台固定 `1280×720`，render scale 目标限 `1.0/1.5/2.0/3.0`；资源密度不改变 VM/save/time，不静默降级。
- `abi_stable` Family API 的 typed 配置由 Manager 保存、核心验证；可选增强关闭不阻止基础播放。窗口/模态请求由 Host 执行，无窗口 Host 明确拒绝。库驻留至进程退出，更新需重启；session 关闭仍须取消请求并 join worker。
- 成功读档/重开始显式返回 `reset_clock`；Host 从 advance 与取帧完成后重新计时。普通帧保留耗时，不扩大 elapsed 上限或静默截断。
- 动态库由适配层安装共享日志桥，静态核心复用宿主订阅器；初始化失败阻止加载。核心日志保留事件、级别、来源和有界 typed 字段，字符串/message/Debug/Display 正常转发，不做桥字段脱敏或白名单；超限计入 `dropped_fields`。

## 验收与记录

- 普通逻辑用 unit/integration test；视听路径使用实际硬件 GPU/音频宿主，允许 headless 或 GPU 虚拟化 Sandbox。软件 adapter、NullAudio、首帧和 synthetic fixture 各只证明其覆盖范围。
- 保留真实 shaping、字体 fallback、完整媒体、资源生命周期和错误诊断。产品完成必须来自实际操作，不用静态报告、窗口存在或测试数量代替。
- Windows 最终验收包括终之空 Classic/Modern 37 路线、FVP/Musica 各一结局；其他平台覆盖代表流程、输入/媒体/存读档/退出。VN 桌面目标 1440p120、移动 1080p60；EMU 按原生节奏分别测核心和 Host，先建同场景基线再定开销门槛。
- 平台为 Windows/Linux/macOS/Android，Editor 仅三桌面；Web/iOS/RPG/TRPG/运行时 AI 延后。Android 默认将游戏导入应用管理目录，静态编译核心。
- 代码提交检查文档链接、fmt、受影响 crate Clippy 和相关测试；阶段末检查活动 workspace build/clippy/test 并运行对应产品。缺依赖、忽略项和失败如实记录；文档改动运行文档检查，不冒报产品验证。
- 文档以中文为主，保留 API/crate 名。只维护当前契约、结论和下一步；删除失效过程页，历史留 Git，不再叠加流水账、审批矩阵或报告 schema。

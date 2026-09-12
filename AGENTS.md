# AstraEngine 实施宪章

## 当前方向

本仓按 2026-09-12 用户确认的全产品重构实施。权威入口是 [总体架构](Docs/product/architecture.md)、[重构契约](Docs/contracts/rebuild.md) 和 [实施状态](Docs/status/implementation-plan.md)。旧页面中与这些决定冲突的 Stage、provider ABI、证据治理、强制 Headless、Qt 和 Luau sandbox 要求不再生效；随相应子系统替换删除旧页面和代码，不创建长期兼容双轨。

单仓按产品分区。先完成 EMU 与 SDK，再完成 Engine/VN、GPUI Editor/Agent、终之空和四平台验收。共享契约先于实现更新。未实现目标不得写成已完成。

## 产品和运行边界

- Engine 是可独立嵌入的 2D 核心。Actor/Component、flat FSM 和任务组合各司其职；不强制所有产品修改通过 FSM action，不要求创建 World 时携带 package。
- Rust 编译期组合和显式依赖注入是 NativeVN 主路径。内部通用 provider/动态 UI ABI 随消费者迁移删除，不新增序列化转发层。
- Runtime 默认逻辑 60 Hz，呈现独立。只保证可靠存读档，不建设通用输入回放、跨平台逐位确定性或全程 hash 链。禁止每帧全量状态克隆和序列化；必要事务保留在编辑、存档和配置边界。
- 任务有明确状态所有者、作用域、取消和完成结果。读档/退出后旧异步结果失效。新演出默认替换同属性旧任务，其他轨道继续。
- `.astra` 是剧情、场景、演出和 UI 的创作权威，使用成熟 lexer/parser/CST/AST 库；图和时间线修改同一源码，布局仅为作者元数据。
- Luau 是可信游戏脚本，可通过宿主使用文件和网络；耗时工作异步。保存显式数据，不保存闭包、协程栈或 native handle。预览 seek 不重复外部副作用。
- Editor 使用 GPUI，独立 GPU 预览窗口，复用真实 VN session。游戏 UI 使用 Yakui，EMU Manager 使用 Slint。
- Editor 使用 ACP 外部 Agent 和 MCP 编辑接口，提供自主/逐批确认模式。统一文档版本、批量编辑和撤销，不另写模型 API Agent 循环。
- EMU 独立于 RuntimeWorld、VN、product package 和 Engine registry。Family 自持 VM、文件、渲染、解码、混音和存档，Host 接收 CPU 最终帧和有界 PCM。
- Family API 使用 abi_stable；可选增强关闭不阻止基础播放。Manager 保存核心/游戏配置，启动时传入并由核心验证。库驻留至进程退出，更新重启 Manager；关闭会话仍须取消请求并等待 worker 结束。
- FVP 保留成熟 RFVP，适配尽量小。上游基线为 0.6.0 revision 304e773387a9920c9db091ec1fd937c717aea949，差异写 MODIFICATIONS.md；共用上游 GlobalSaveDataV1/RFVG，不复制 hosted 存档结构。
- Minori 使用可选 SDK，共享底层能力不要求创建 Engine session。短期采用核心自有存档，长期才做原版兼容。成熟核心无需采用 SDK。
- Minori GARbro scheme 保留纯 Rust 两阶段 NRBF reader：收集对象再解析引用，未知/断裂/重复/越界及不符合图约束的输入明确失败，不使用 BinaryFormatter 或启发式 fallback。
- 保留真实文字 shaping、字体 fallback、媒体播放、资源生命周期和错误诊断；不能以矩形、首帧或 synthetic fixture 冒充完整产品。
- 平台范围为 Windows/Linux/macOS/Android，Editor 仅三桌面；Web/iOS/RPG/TRPG/运行时 AI 本轮延后。Android 默认导入游戏到应用管理目录，静态编译核心。

## 代码、数据与协作

- 每个实例独占 Git worktree、branch、target 和服务。开始前检查归属；禁止在其他实例 worktree 编辑/编译/测试。只清理自己创建且不再使用的产物。并行 agent 通过独立提交整合。
- 成熟 crate 优先；按真实消费者、平台隔离和职责拆分。合并纯转发 crate，不为数量目标新增抽象。lib.rs 薄 facade；接近 400–600 行拆模块。
- Rust 使用 idiomatic 命名与 typed API；Rust 类型为 schema 真源。跨平台脚本用 Python，命令示例用 bash/sh。
- 日志使用 tracing/astra-observability，库不初始化全局 sink；稳定事件字段。secret、商业正文和未经审计的 payload 不写日志。私有路径不写可提交报告。
- 保留容器版本、长度/边界、唯一 section、内容完整性和明确错误。postcard struct 不使用导致二进制不可读的 skip_serializing_if。读取失败不覆盖原存档。
- 商业游戏源、截图、转换数据和私有包只在 ignored 工作区；保留许可证和上游修改说明。终之空公开仓库只含工具/模板/非商业测试，不能提交商业 payload。既有商业存档永不误覆盖。
- Git 提交用短祈使句。常规实现与本地提交已由重构任务授权；发布、外发或 destructive 操作遵循用户授权范围。

## 测试和状态

- 普通逻辑用 Rust unit/integration test，无需启动 Headless；需要视听时才创建 GPU/音频宿主。纯代码 doctest 可恢复。删除强制全测试 Headless 和具名 tolerance/evidence 审批要求。
- 每次提交至少检查文档链接、fmt、受影响 crate 的 clippy 和相关测试；阶段末执行全部活动 workspace 的 build/clippy/test。平台缺依赖或资源须准确记录，不能以跳过冒充通过。
- 开发 Agent 辅助操作真实产品、查看截图和音频分析。产品完成须来自实际运行，不以静态报告、窗口存在或局部 fixture 代替。
- Windows 验证终之空 Classic/Modern 37 路线、FVP/Minori 各一结局；其余平台验证代表流程、输入/媒体/存读档/退出与性能。
- VN 桌面目标 1440p120、移动 1080p60；EMU 按原生速率，分别测核心与 Host。固定场景记录实际硬件，未测保持未验收。
- 文档中文主体，API/crate 名保留英文。现行文档写实际接口与状态，删除失效治理内容，历史留 Git。文档检查仅做链接和明确内容卫生，不强制 Stage 状态矩阵或报告 schema。

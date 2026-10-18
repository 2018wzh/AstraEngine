# 总体架构

AstraEngine 是单仓中的共享 2D 核心和产品组合。具体公共边界见 [重构契约](../contracts/rebuild.md)，实际进度见 [实施计划](../status/implementation-plan.md)。

## 产品分工

| 产品 | 职责 |
| --- | --- |
| Engine | 可独立嵌入的场景对象、任务、时间、输入、渲染、文字、媒体和资源生命周期 |
| AstraVN | 剧情游标、角色数据、演出编排、可信 Luau、标准系统与存读档 |
| AstraEditor | GPUI 文本/图/时间线/Inspector、独立预览窗口、ACP Agent 与 MCP 编辑能力 |
| AstraEMU | Slint Manager、统一 C ABI、原生 Embedded 核心、CPU/GPU FrameSurface 与可选 SDK |
| 共享底层库 | Engine 与 SDK 按需使用的文字、图像、音视频、绘制和字节源 |

## 数据与生命周期

.astra 经成熟 CST/AST 前端编译为 Story、Scene、Sequence、角色预设和 UI。图形编辑回写同一源文件。VN session 拥有游戏数据与剧情，场景实例拥有呈现对象与局部任务。Player 和预览使用相同 session 实现。

Runtime 采用 60 Hz 逻辑与独立呈现；FSM、剧情执行器、轨道求值共用任务/作用域机制，不强迫互相转换。普通 Rust typed API 组合后端，不使用内部产品动态 ABI 转发。保存显式运行状态，不记录全量事件历史。

AstraEMU 的目标为同进程单活动游戏。完整原生核心自主管理 VM、调度、资源、渲染、游戏 UI、音视频、输入语义和原生存档，通过统一 C ABI 交付完整 CPU 或 GPU FrameSurface；Manager 统一负责滤镜、视口、界面及 Present。核心优先保留自己的 GPU Device，不强制公共 advance、Host PCM、Mixer 或 VFS。Dynamic/Static 共用接口，动态库驻留至进程退出，关闭会话仍须回收 worker 与资源。

核心集成区分自有与外部：Musica/CMVS 可将适配能力直接嵌入引擎，并与 SDK 深度集成，不要求另设薄 Family 包装；外部核心保留薄适配和原 VM、渲染、媒体、调度及存档。SDK 抽取实际共用能力，各引擎保留格式与游戏语义；统一 ABI、资源所有权及 Manager 最终呈现边界不变。

逻辑舞台、渲染尺寸、窗口输出及内容区域分别表达。核心声明渲染配置的支持范围、默认值与实际生效尺寸；Manager 展示并保存，默认在启动时应用。窗口缩放只改变显示适配，不修改游戏逻辑或暗改渲染配置。版本化图形扩展协商共享句柄和双向同步，公共 ABI 不传递 Rust 图形对象。

Manager 使用 Slint，重设计采用精炼 Material 3，在同一窗口切换游戏库与游戏视图；游戏输入区不被宿主工具栏侵占。显示滤镜作用于完整游戏帧，Manager UI 在之后绘制。扩展通过安全点 Hook 调用固定线程中的共享串行 Luau VM，原生取消和错误界面不排在阻塞任务之后。具体实现与验收范围见实施计划；这些是目标契约，不代表旧 Family v13 已完成迁移。

## 交付边界

先 EMU/SDK，再 Engine/VN、Editor/Agent 和终之空。Editor 覆盖三桌面；VN/EMU 覆盖 Windows/Linux/macOS/Android。终之空为本地转换项目与私有包，Classic/Modern 全部 37 路线。Web/iOS、RPG/TRPG、运行时 AI、完整 Live2D 与通用节点编程留后续路线。

本次 EMU 首先交付 Windows：RFVP 原生 CPU 完整路径、Musica 文字 Hook、RFVP 首个 GPU 交付后端，以及其余参与核心的原生整合。本轮只做 Release 人工可玩验收，不要求结局；CMVS 的深集成设计获准，但本轮不改其独立工作树。Linux/macOS/Android 在后续平台阶段分别验收。新接口不维护核心自行 Present 的第二条 Manager 接入路径，上游独立程序继续作为行为对照。

普通分层测试与产品验收分开；本轮 EMU 由用户操作 Release 版本，不运行自动输入、截图或关闭驱动，不设置 evidence 审批体系。

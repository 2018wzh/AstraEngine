# 总体架构

AstraEngine 是单仓中的共享 2D 核心和产品组合。具体公共边界见 [重构契约](../contracts/rebuild.md)，实际进度见 [实施计划](../status/implementation-plan.md)。

## 产品分工

| 产品 | 职责 |
| --- | --- |
| Engine | 可独立嵌入的场景对象、任务、时间、输入、渲染、文字、媒体和资源生命周期 |
| AstraVN | 剧情游标、角色数据、演出编排、可信 Luau、标准系统与存读档 |
| AstraEditor | GPUI 文本/图/时间线/Inspector、独立预览窗口、ACP Agent 与 MCP 编辑能力 |
| AstraEMU | 独立 Slint Manager、薄 Family API、FVP/Minori、自主核心和可选 SDK |
| 共享底层库 | Engine 与 SDK 按需使用的文字、图像、音视频、绘制和字节源 |

## 数据与生命周期

.astra 经成熟 CST/AST 前端编译为 Story、Scene、Sequence、角色预设和 UI。图形编辑回写同一源文件。VN session 拥有游戏数据与剧情，场景实例拥有呈现对象与局部任务。Player 和预览使用相同 session 实现。

Runtime 采用 60 Hz 逻辑与独立呈现；FSM、剧情执行器、轨道求值共用任务/作用域机制，不强迫互相转换。普通 Rust typed API 组合后端，不使用内部产品动态 ABI 转发。保存显式运行状态，不记录全量事件历史。

EMU Family 自持原引擎 VM、文件与存档，Host 只管理配置、输入、窗口、CPU 最终帧、PCM 与可选文本服务。FVP 保持成熟 RFVP，小型薄适配；Minori 使用可选 SDK。动态核心驻留至进程退出，移动端编译入应用。

## 交付边界

先 EMU/SDK，再 Engine/VN、Editor/Agent 和终之空。Editor 覆盖三桌面；VN/EMU 覆盖 Windows/Linux/macOS/Android。终之空为本地转换项目与私有包，Classic/Modern 全部 37 路线。Web/iOS、RPG/TRPG、运行时 AI、完整 Live2D 与通用节点编程留后续路线。

普通分层测试与开发 Agent 真实操作共同验收，按实际设备测性能，不设置 evidence 审批体系。

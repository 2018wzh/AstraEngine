# AstraEditor

AstraEditor 使用 GPUI，产品目标是达到 Unreal Editor 同等级的易用性，主要服务 NativeVN 的 2D 分层场景、剧情、演出和 UI 创作。这个目标尚未达成。旧 Qt/QML、动态 Runtime Provider 切换和嵌入式模型请求循环不再是实施方向。

## 人工创作主流程

用户打开项目后，从 Content Browser 查找源文件和资源，在 Outliner、Graph 或 Timeline 选择对象，再通过 Details 修改同一份 `.astra`。源码是唯一权威；面板选择、布局和展开状态属于 Editor 状态。人工流程不依赖 Agent。

Save、Undo、Redo、脏状态和关闭确认共用文档事务。编译诊断和对象选择使用 compiler source map/CST source ID 定位。Play 运行真实 Player，构建中、运行、停止和失败必须可区分；缺少宿主能力时不显示虚假的 Pause 或 Seek 控件。

## 实现边界

版本化编辑、GPUI 文本输入、文件保存冲突、ACP/MCP 共用事务桥和预览进程管理已接入。工程浏览、联动 Details、布局和图/演出视图正在完善。当前还不能把命令列表当作完整图形节点编辑器，或把 keyframe 文本字段当作完整曲线编辑器。

外部 Agent 通过 ACP 连接，模型配置由 Agent 管理。自主和逐批确认模式经过相同版本、范围和取消检查。MCP 编辑访问当前未保存的文档，不绕过 UI 审批。

## 参考与入口

交互参考 Epic 的 [Editor Interface](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-editor-interface)、[Content Browser](https://dev.epicgames.com/documentation/en-us/unreal-engine/content-browser-in-unreal-engine) 和 [Play & Simulate](https://dev.epicgames.com/documentation/en-us/unreal-engine/ineditor-testing-play-and-simulate-in-unreal-engine)：统一选择驱动 Details、可恢复工作区、可搜索资源、明确的运行模式。

契约见 [重构契约](../contracts/rebuild.md)，操作和增量测试见 [Editor 手册](../../Editor/README.md)，结构见 [Shell](../implementation/editor/shell.md)，总体状态见 [实施计划](../status/implementation-plan.md)。

# AstraVN

AstraVN 是原生 VN 垂直模块，不是 EMU 或其他玩法的父类。代码位于 `Engine/Source/Modules/AstraVN/`；目标边界见[重构契约](../contracts/rebuild.md)，实际进度见[实施状态](../status/implementation-plan.md)。

## 会话与所有权

`astra-vn::VnSession` 持有剧情、变量和系统状态，组合 `astra-runtime::EngineSession`。公共运行层拥有场景、任务、时钟和作用域；`VnRuntime` 只实现 VN 业务。逻辑默认 60 Hz，呈现由 Player 独立推进，普通剧情操作不经动态 provider 或转发 FSM。

保存显式数据与任务状态，不保存 Luau 闭包、协程栈或 native handle。读档先验证容器及状态，失败保留当前会话；成功后取消旧作用域，拒绝迟到的异步结果。回想、语音重听和读状态是 VN 功能，不代表建设通用输入回放或逐位确定性。

旧 runtime-provider crate、Factory/FFI 转发、`native_vn_descriptor()` 和内部 gameplay/UI 动态 ABI 消费链已删除。机制插件与 EMU Family 的独立 ABI 不属于这条旧链路。

## 按真实职责划分

| Crate | 职责 |
| --- | --- |
| `astra-vn-script` | `.astra` 前端、CST/AST、编译、source map、标准命令和系统 UI 声明 |
| `astra-vn-core` | 剧情游标、变量、选择、call/return、等待、读状态、backlog 与系统状态 |
| `astra-vn-policy` | Luau 绑定、显式动作/IO continuation、预算、可保存状态与包内源码 |
| `astra-vn-presentation` | 逻辑舞台、图层、镜头、文字、音视频与演出任务 |
| `astra-vn-package` | `vn.*` 包 section、配置、扩展与存档相关契约 |
| `astra-vn-editor` | 版本化文档、批量编辑、source ID 投影与 Graph/Timeline 事务 |
| `astra-vn` | typed VN 产品会话入口 |

功能 crate 使用窄依赖；共享类型放在实际拥有者，不为 facade 数量新增转发层。

## 创作与播放

`.astra` 是剧情、场景、演出和 UI 的唯一创作来源。Graph/Timeline 修改同一源码；选择与布局是作者元数据，不是另一份运行时模型。可信 Luau 的宿主接口与当前限制见[Luau](../implementation/luau-policy.md)。

Player 和 Editor 预览使用真实 VN 会话。对白、选择、auto/skip、系统页、存读档、媒体、回想及演出均需走共享产品路径；声明、局部 fixture 或最短生命周期检查不能代替完整游戏行为。

## 查阅入口

- [`.astra` 前端](../implementation/astra-grammar-ir.md)与[脚本规格](astra-vn-script.md)
- [呈现模型](astra-vn-presentation-model.md)、[标准命令](astra-vn-standard-commands.md)与[系统 UI](astra-vn-system-ui-profile.md)
- [版本化编辑](../contracts/rebuild.md#创作与编辑)与[Editor 手册](../../Editor/README.md)
- [开发与实际验证](../manual/development.md)：终之空 Classic/Modern 37 路线、设备与性能结果仍按实际状态记录

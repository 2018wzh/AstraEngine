# ADR 0002: Editor UI First Stage Uses Qt

Status: Accepted

## Context

AstraEditor 需要 dockable 工作台、资产浏览、脚本/Graph/Timeline/FilterGraph 编辑、Inspector、Runtime Debugger、AI Review Queue 和 Compatibility Inspector。

## Decision

第一阶段 Editor UI 使用 Qt 作为 shell 和工具面板框架。引擎渲染输出嵌入 Scene View。Editor 只访问 Runtime public DTO、ServiceRegistry 和 ExtensionRegistry，不访问内部 native object。

## Consequences

- Runtime 不依赖 Editor。
- Editor panel 可由动态模块注册。
- UI 技术不决定 Runtime、Actor、StateMachine 或 FilterGraph 的公共接口。

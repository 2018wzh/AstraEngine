# AstraEditor Module

AstraEditor 使用 Qt/QML + Rust core。Editor 是 creator workflow 和 debugger，不是 packaged runtime 的前置条件。

## V1 面板

- Project Wizard / Template Browser
- Content Browser / Import Wizard
- Inspector / Details Panel
- Script Editor
- Graph Editor
- Timeline Editor
- FilterGraph / AudioGraph Editor
- PIE Viewport
- Runtime Debugger
- Save/Replay Inspector
- Package / Release Gate Panel
- AI Review Queue / Trusted Session Audit

## Editor Runtime Session

PIE 使用同一 RuntimeWorld public API。Editor 通过 debug session 查看 Actor、Component、StateMachine、EventQueue、AwaitToken、ScriptSnapshot、FilterGraph、AudioGraph 和 ReleaseReport。

## Luau Policy Visualization

Luau 策略像可视化基类，Graph/Timeline 是创作者派生层。策略包必须暴露节点、端口、Inspector 控件、Timeline track、preview input/output、source map 和 diagnostics；Editor 默认按段落/场景级编辑，复杂 Luau 内部逻辑显示为策略节点。

PIE/Preview 可以刷新 Luau 策略；发布 runtime 不支持策略热重载。

## Trusted Session

项目授权后，AI 可以直写 canonical source、Luau 策略和 Graph/Timeline 派生层。Editor UI 必须能查看、回滚和解释每次 patch、graph diff、audit event 和 release check。

## UE 级创作者工作流

v1 面板必须覆盖空状态、加载中、错误、可编辑、只读和 release blocked 状态。Project Wizard、Content Browser、Script、Graph、Timeline、Inspector、PIE、Debugger、Package Gate 和 AI Review Queue 的数据来源、操作和验收见 [Editor Workflow Blueprint](../implementation/editor-workflow.md)。

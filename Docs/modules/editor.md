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

## Trusted Session

Trusted session 可让 AI 直写 canonical source，但必须有授权范围、patch、audit event 和 undo checkpoint。Editor UI 必须能查看、回滚和解释每次直写。

# AstraEditor Module

AstraEditor 使用 Qt/QML + Rust core。Editor 是 creator workflow 和 debugger，不是 packaged runtime 的前置条件。Editor shell 不绑定单一玩法类型，项目必须通过 `ProductRuntimeProvider` 显式选择 AstraVN、AstraEMU 或后续 AstraRPG。

## V1 面板

- Project Wizard / Template Browser
- Project Settings / Plugin Manager
- Command Palette
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

## Runtime Provider Switching

Project Wizard、Project Settings、Plugin Manager、PIE、Debugger 和 Release Gate 都读取 selected `ProductRuntimeProvider` 的 `RuntimeEditorMetadata`。公共 shell 保持不变；Script、Graph、Timeline、Map、Quest、legacy trace 等玩法面板由 provider metadata 决定。

NativeVN 当前提供 `.astra` Script、VN Graph、Timeline、System UI 和 Luau policy surface。AstraEMU/AstraRPG 仍是 planned peer runtime；Editor 只预留 case profile、legacy pack VFS、family trace、Map、Quest、Battle/Party/Inventory 等接入边界，不把它们写成已实现 UI。

## Editor Runtime Session

PIE 使用同一 RuntimeWorld public API，并由 selected gameplay runtime provider 打开 Game target session。Editor 通过 debug session 查看 Actor、Component、StateMachine、EventQueue、AwaitToken、ScriptSnapshot、FilterGraph、AudioGraph、RuntimeEditorMetadata 和 ReleaseReport。

## Plugin Manager

Plugin Manager 使用 ExtensionRegistry 报告，不直接加载私有 UI。它显示 load phase、extension point、dependency graph、enablement、权限、冲突、packaged 裁剪和 release check。菜单、面板、资产类型、Graph node、Timeline track、Inspector widget 和 release check 都必须能跳到 descriptor source 或 diagnostic source。

## Luau Policy Visualization

Luau 策略像可视化基类，Graph/Timeline 是创作者派生层。策略包必须暴露节点、端口、Inspector 控件、Timeline track、preview input/output、source map 和 diagnostics；Editor 默认按段落/场景级编辑，复杂 Luau 内部逻辑显示为策略节点。

PIE/Preview 可以刷新 Luau 策略；发布 runtime 不支持策略热重载。

## Trusted Session

项目授权后，AI 可以直写 canonical source、Luau 策略和 Graph/Timeline 派生层。Editor UI 必须能查看、回滚和解释每次 patch、graph diff、audit event 和 release check。

## UE 级创作者工作流

v1 面板必须覆盖空状态、加载中、错误、可编辑、只读和 release blocked 状态。Project Wizard、Project Settings、Plugin Manager、Command Palette、Content Browser、Script、Graph、Timeline、Inspector、PIE、Debugger、Package Gate 和 AI Review Queue 的数据来源、操作和验收见 [Editor Workflow Blueprint](../implementation/editor-workflow.md)。

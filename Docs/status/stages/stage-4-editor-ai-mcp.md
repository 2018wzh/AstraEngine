# Stage 4 Editor + AI/MCP Work

Stage 4 建立 creator workflow 和 AI/MCP 闭环。Editor 不改变 EngineCore 边界；Runtime AI 和 Editor AI 都必须可审计、可回滚，并通过 provider-free replay。本页是 planned target 清单，不表示实现已经存在。

## S4-EDITOR-01 Qt/QML shell 与 Project Wizard

**ID:** `S4-EDITOR-01`

**Goal:** AstraEditor 提供 Qt/QML shell、Project Wizard、workspace loading 和基础 panel 布局。

**Depends On:** `Docs/modules/editor.md`、`S3-SAMPLE-01`

**Target Paths:** `Editor/Source/App/astra-editor/`、`Editor/Source/Bridge/astra-editor-bridge/`、`Editor/Tests/project_wizard.rs` planned target

**Steps:**

1. 建立 Qt/QML shell 和 Rust bridge 进程边界。
2. 实现 Project Wizard，创建 AstraVN project manifest、source tree 和 sample policy binding。
3. 加载 project 后显示 Content Browser、Graph、Timeline、Inspector 和 Package panel 的空状态。
4. 编写 wizard create/load smoke test。

**Done Evidence:** Project Wizard 到 project load 的 creator workflow 可重复运行，不写入 EngineCore 私有状态。

**Linked Test IDs:** `T-S4-EDITOR-01`

## S4-EDITOR-02 PIE runtime bridge

**ID:** `S4-EDITOR-02`

**Goal:** PIE 通过同一 Runtime public API 启动 sample、驱动 input、暂停、恢复和读取 diagnostics。

**Depends On:** `S4-EDITOR-01`、`S3-CORE-03`

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/pie.rs`、`Editor/Tests/pie_bridge.rs` planned target

**Steps:**

1. 定义 PIE session lifecycle：create、launch、tick、pause、resume、stop。
2. 把 Editor input 转成 Runtime PlayerInput，不直接操作 Actor 指针。
3. 暴露 TickReport、diagnostics、scenario cursor 和 presentation hash。
4. 编写 PIE launch、pause/resume 和 diagnostic forwarding 测试。

**Done Evidence:** PIE 使用 Runtime API，不依赖 private runtime state。

**Linked Test IDs:** `T-S4-EDITOR-02`

## S4-PLUGIN-01 Plugin Manager 与 extension diagnostics

**ID:** `S4-PLUGIN-01`

**Goal:** Editor 提供 Plugin Manager、Project Settings、Command Palette 和 extension diagnostics，覆盖启用禁用、依赖图、冲突解释和 diagnostic jump。

**Depends On:** `S1-PLUGIN-01`、`Docs/implementation/provider-plugin-api.md`、`S4-EDITOR-01`

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/plugin_manager.rs`、`Editor/Source/App/astra-editor/qml/PluginManager.qml`、`Editor/Tests/plugin_manager.rs` planned target

**Steps:**

1. 读取 `ExtensionRegistrationReport`，显示 load phase、extension point、dependency graph、permission 和 packaged 裁剪。
2. 支持 project enable/disable、provider binding、conflict resolution 和 command palette visibility。
3. 对 Editor panel、menu command、asset type、graph node、timeline track、Inspector widget 和 release check 提供 diagnostic jump。
4. 编写 missing dependency、extension conflict、disable impact 和 report evidence 测试。

**Done Evidence:** Plugin Manager 输出 `editor.plugin_manager`、`plugin.extension_registry` 和 `plugin.dependency_graph` evidence。

**Linked Test IDs:** `T-S4-PLUGIN-01`

## S4-EDITOR-03 Inspector 与 Debugger

**ID:** `S4-EDITOR-03`

**Goal:** Inspector 和 Debugger 读取 PropertySystem metadata、Runtime debug session、StateMachine trace 和 source map。

**Depends On:** `S1-PROP-01`、`S4-EDITOR-02`

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/inspector.rs`、`Editor/Source/Bridge/astra-editor-bridge/src/debugger.rs`、`Editor/Tests/inspector_debugger.rs` planned target

**Steps:**

1. 通过 PropertySystem metadata 构建 Inspector field model。
2. 通过 RuntimeDebugSession 查询 actor/component、blackboard、state machine 和 event trace。
3. 把 trace span 映射回 `.astra` source map。
4. 编写 property render、state trace 和 source span lookup 测试。

**Done Evidence:** Inspector 不持有 Runtime 内部指针，Debugger 可定位到 source map。

**Linked Test IDs:** `T-S4-EDITOR-03`

## S4-EDITOR-04 Graph/Timeline 编辑闭环

**ID:** `S4-EDITOR-04`

**Goal:** Graph/Timeline 可编辑 AstraVN metadata，并回写到 `.astra`、Luau metadata 或 policy override。

**Depends On:** `S3-EDIT-01`、`S4-EDITOR-03`

**Target Paths:** `Editor/Source/App/astra-editor/qml/GraphView.qml`、`Editor/Source/App/astra-editor/qml/TimelineView.qml`、`Editor/Tests/graph_timeline_edit.rs` planned target

**Steps:**

1. 读取 command id、Graph node metadata 和 Timeline track metadata。
2. 支持移动节点、调整 transition duration、编辑 fence 和保存 override。
3. 回写后重新编译 CompiledStory，并保持 source map identity。
4. 编写 edit-save-recompile roundtrip 测试。

**Done Evidence:** Editor 可视修改不会产生第二套 runtime model。

**Linked Test IDs:** `T-S4-EDITOR-04`

## S4-EDITOR-05 Package/Release Gate panel

**ID:** `S4-EDITOR-05`

**Goal:** Package panel 调用同一 CLI validator，显示 release report、blocking diagnostic 和可复现命令。

**Depends On:** `S2-GATE-01`、`S4-EDITOR-02`

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/package_panel.rs`、`Editor/Source/App/astra-editor/qml/ReleaseGatePanel.qml`、`Editor/Tests/release_gate_panel.rs` planned target

**Steps:**

1. 从 Editor 调用 package validate，不实现第二套 gate。
2. 显示 check id、status、diagnostic、source span 和 report path。
3. 支持从 failed scenario 跳转到 source map 或 scenario action。
4. 编写 report render、blocking check 和 command copy 测试。

**Done Evidence:** Editor、CLI、CI 读取同一 release report schema。

**Linked Test IDs:** `T-S4-EDITOR-05`

## S4-AI-01 Runtime AI committed output

**ID:** `S4-AI-01`

**Goal:** Runtime AI 输出必须通过 IntentValidator，并固化进 save/replay。

**Depends On:** `S1-SAVE-01`、`Docs/contracts/ai-mcp.md`

**Target Paths:** `Engine/Source/Developer/astra-ai/src/runtime_ai.rs`、`Engine/Source/Developer/astra-ai/src/intent_validator.rs`、`Engine/Source/Developer/astra-ai/tests/runtime_ai_replay.rs` planned target

**Steps:**

1. 定义 RuntimeAiRequest、Intent、IntentValidator 和 CommittedAiOutput。
2. 只允许 validator 通过的 intent 进入 Runtime event queue。
3. 把 committed output 写入 save/replay section。
4. 编写 provider unavailable replay、invalid intent blocked 和 committed output hash 测试。

**Done Evidence:** replay 不重新请求 provider，AI output 仍可复现。

**Linked Test IDs:** `T-S4-AI-01`

## S4-AI-02 Editor Copilot、Trusted session 与 Review Queue

**ID:** `S4-AI-02`

**Goal:** Editor Copilot 支持 Review Queue 和 Trusted session，所有写入生成 patch、audit event 和 undo checkpoint。

**Depends On:** `S4-EDITOR-05`

**Target Paths:** `Engine/Source/Developer/astra-ai/src/editor_copilot.rs`、`Engine/Source/Developer/astra-ai/src/trusted_session.rs`、`Engine/Tests/AI/editor_copilot.rs` planned target

**Steps:**

1. 定义 TrustedSessionScope，包含 project、path range、operation type 和 expiration。
2. 未授权写入进入 Review Queue，授权写入生成 patch 和 undo checkpoint。
3. 所有 AI 操作写 audit event，记录 provider、prompt hash、tool call 和 affected paths。
4. 编写 unauthorized queued、trusted write、undo checkpoint 和 audit event 测试。

**Done Evidence:** Copilot 不能绕过权限、审计和回滚。

**Linked Test IDs:** `T-S4-AI-02`

## S4-MCP-01 MCP tool descriptor 与 capability protocol

**ID:** `S4-MCP-01`

**Goal:** MCP tool 只能通过声明过的 project、Runtime、Editor 和 Release Gate capability 访问能力。

**Depends On:** `S4-AI-02`、`Docs/contracts/ai-mcp.md`

**Target Paths:** `Engine/Source/Developer/astra-mcp/src/tool_descriptor.rs`、`Engine/Source/Developer/astra-mcp/src/capability.rs`、`Engine/Tests/MCP/capability_protocol.rs` planned target

**Steps:**

1. 定义 MCP tool descriptor、permission scope、audit fields 和 result schema。
2. 把 Runtime、Editor、Package 和 Release Gate 操作映射到 capability check。
3. 拒绝未授权 path、operation 和 session。
4. 编写 capability allowed、denied 和 audit propagation 测试。

**Done Evidence:** MCP 不能绕过 Editor/CLI 的 release gate 和 permission policy。

**Linked Test IDs:** `T-S4-MCP-01`

## S4-GATE-01 Provider-free replay 与 audit gate

**ID:** `S4-GATE-01`

**Goal:** Release Gate 覆盖 Runtime AI、Editor Copilot、MCP audit 和 provider-free replay。

**Depends On:** `S4-AI-01`、`S4-AI-02`、`S4-MCP-01`

**Target Paths:** `Engine/Source/Developer/astra-release/src/ai_gate.rs`、`Engine/Source/Developer/astra-release/tests/ai_mcp_gate.rs` planned target

**Steps:**

1. 增加 `ai.provider_free_replay`、`ai.audit_complete`、`mcp.permission_policy` gate check。
2. 校验 save/replay 中 committed AI output 完整，且不需要 provider。
3. 校验 Trusted session audit chain 和 Review Queue disposition。
4. 编写 gate pass、missing audit blocked 和 provider replay blocked 测试。

**Done Evidence:** v0.4 gate 能阻断缺失 audit 或 provider-free replay 失败的 package。

**Linked Test IDs:** `T-S4-GATE-01`

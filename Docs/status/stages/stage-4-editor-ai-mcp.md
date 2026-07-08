# Stage 4 Editor + AI/MCP Work

Stage 4 建立 creator workflow 和 AI/MCP 闭环。Editor 不改变 EngineCore 边界；Runtime AI 和 Editor AI 都必须可审计、可回滚，并通过 provider-free replay。本页是 `REOPENED_SPEC` 清单，不表示实现已经存在。本轮重开后，ONNX ModelBundle、Context Pack、generated artifact 和 MCP package access 必须复用 [Asset VFS](../../contracts/asset-vfs.md)，不能继续以 loose sidecar 或私有 package source 表达。

## S4-EDITOR-01 Qt/QML shell 与 Project Wizard

**ID:** `S4-EDITOR-01`

**Goal:** AstraEditor 提供 Qt/QML shell、Project Wizard、workspace loading 和基础 panel 布局。

**Depends On:** `Docs/modules/editor.md`、`S3-SAMPLE-01`

**Target Paths:** `Editor/Source/App/astra-editor/`、`Editor/Source/Bridge/astra-editor-bridge/`、`Editor/Tests/project_wizard.rs` planned target

**Steps:**

1. 建立 Qt/QML shell 和 Rust bridge 进程边界。
2. 实现 Project Wizard，读取 `RuntimeEditorMetadata`，创建 NativeVN project manifest、source tree 和 sample policy binding，并保留 AstraEMU/AstraRPG planned provider 的不可用诊断。
3. 加载 project 后显示 Content Browser、Graph、Timeline、Inspector 和 Package panel 的空状态。
4. 编写 wizard create/load smoke test。

**Done Evidence:** Project Wizard 到 project load 的 creator workflow 可重复运行，不写入 EngineCore 私有状态；runtime provider metadata 决定可见模板和面板。

**Linked Test IDs:** `T-S4-EDITOR-01`

## S4-EDITOR-02 PIE runtime bridge

**ID:** `S4-EDITOR-02`

**Goal:** PIE 通过同一 Runtime public API 启动 sample、驱动 input、暂停、恢复和读取 diagnostics。

**Depends On:** `S4-EDITOR-01`、`S3-CORE-03`

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/pie.rs`、`Editor/Tests/pie_bridge.rs` planned target

**Steps:**

1. 定义 PIE session lifecycle：create、launch、tick、pause、resume、stop。
2. PIE launch request 必须携带 Game target id、runtime provider id 和 profile。
3. 把 Editor input 转成 Runtime PlayerInput，不直接操作 Actor 指针。
4. 暴露 TickReport、diagnostics、scenario cursor 和 presentation hash。
5. 编写 PIE launch、pause/resume 和 diagnostic forwarding 测试。

**Done Evidence:** PIE 使用 Runtime API，不依赖 private runtime state。

**Linked Test IDs:** `T-S4-EDITOR-02`

## S4-PLUGIN-01 Plugin Manager UI 与 extension diagnostics

**ID:** `S4-PLUGIN-01`

**Goal:** Editor 提供 Plugin Manager、Project Settings、Command Palette 和 extension diagnostics UI，读取并修改 Stage 1/2 的 extension registry、dependency graph 和 provider binding。

**Depends On:** `S1-PLUGIN-03`、`S2-PLUGIN-GATE-01`、`Docs/implementation/provider-plugin-api.md`、`S4-EDITOR-01`

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/plugin_manager.rs`、`Editor/Source/App/astra-editor/qml/PluginManager.qml`、`Editor/Tests/plugin_manager.rs` planned target

**Steps:**

1. 读取 Stage 1/2 产出的 `ExtensionRegistrationReport`、`plugin.extension_registry` 和 `plugin.dependency_graph`。
2. 支持 project enable/disable、provider binding、conflict resolution 和 command palette visibility，并写回 project enablement。
3. 对 Editor panel、menu command、asset type、graph node、timeline track、Inspector widget 和 release check 提供 diagnostic jump。
4. 不实现第二套 dependency graph、provider selection 或 packaged eligibility 逻辑。
5. 编写 missing dependency、extension conflict、disable impact 和 report evidence 测试。

**Done Evidence:** Plugin Manager UI 输出 `editor.plugin_manager` evidence，并显示 Stage 1/2 的 `plugin.extension_registry` 和 `plugin.dependency_graph` evidence。

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

## S4-AI-01 Runtime Director committed output

**ID:** `S4-AI-01`

**Goal:** Runtime Director 通过受限 `McpAiSession` 调用模型，输出 typed Intent，并固化进 save/replay。

**Depends On:** `S1-SAVE-01`、`S4-MCP-01`、`Docs/contracts/ai-mcp.md`

**Target Paths:** `Engine/Source/Developer/astra-ai/src/runtime_ai.rs`、`Engine/Source/Developer/astra-ai/src/intent_validator.rs`、`Engine/Source/Developer/astra-ai/tests/runtime_ai_replay.rs` planned target

**Steps:**

1. 定义 `McpAiSessionScope`、`RuntimeAiRequest`、typed `RuntimeAiIntent`、`IntentValidator` 和 `CommittedAiOutput`。
2. Runtime 只通过 session 请求模型，不直接持有 provider trait object。
3. 只允许 validator 通过的 dialogue、choice、presentation beat 和 memory update 进入 Runtime event queue。
4. 把 committed output 写入 save/replay section。
5. 编写 provider unavailable replay、invalid intent blocked、startup provider missing blocked 和 committed output hash 测试。

**Done Evidence:** replay 不重新请求 provider；Live AI provider 缺失时启动阻断，不静默换 provider。

**Linked Test IDs:** `T-S4-AI-01`

## S4-AI-02 AI provider profiles

**ID:** `S4-AI-02`

**Goal:** OpenAI、Ollama 和 ComfyUI 作为第一方 provider profile 接入 Editor 和 MCP host，默认禁用，显式绑定。

**Depends On:** `S1-PLUGIN-01`、`Docs/implementation/ai-provider-profiles.md`

**Target Paths:** `Engine/Plugins/Providers/astra-ai-openai/`、`Engine/Plugins/Providers/astra-ai-ollama/`、`Engine/Plugins/Providers/astra-ai-comfyui/`、`Engine/Source/Developer/astra-ai/tests/provider_profiles.rs` planned target

**Steps:**

1. 定义 `AiProviderProfile`、`AiCapabilityReport`、`SecretHandle`、data egress 和 runtime eligibility。
2. OpenAI profile 声明 cloud LLM、embedding、tool call、network egress 和 secret handle。
3. Ollama profile 声明 local endpoint、LLM、embedding 和无 cloud secret。
4. ComfyUI profile 声明 workflow host 和 asset draft sidecar，默认 Editor-only。
5. 编写 fake server、workflow fixture、profile gate 和 real smoke opt-in 测试。

**Done Evidence:** 第一方 provider 通过契约测试；真实服务 smoke 不进入默认 CI。

**Linked Test IDs:** `T-S4-AI-02`

## S4-AI-ONNX Packaged ONNX Runtime provider and ModelBundle

**ID:** `S4-AI-ONNX`

**Goal:** `astra-ai-onnx` 作为一方 provider profile 接入 Editor 和 MCP host；模型、ONNX Runtime reduced runtime、Web runtime adapter、tokenizer、pipeline config 和 custom op sidecar 通过 cook/package 成为 ModelBundle，并由 package/VFS section ref 读取。

**Depends On:** `S2-PACKAGE-01`、`S2-ASSET-01`、`S2-VFS-01`、`S2-PLUGIN-GATE-01`、`S4-AI-01`、`S4-AI-02`、`Docs/implementation/ai-provider-profiles.md`、`Docs/implementation/package-save.md`、`Docs/implementation/asset-vfs.md`

**Target Paths:** `Engine/Plugins/Providers/astra-ai-onnx/`、`Engine/Source/Developer/astra-ai/src/model_bundle.rs`、`Engine/Source/Developer/astra-ai/tests/onnx_model_bundle.rs` planned target

**Steps:**

1. 定义 `PackagedOnnxRuntime` provider profile、`ai.model_bundle_manifest`、ModelBundle pipeline、VFS mount id、section refs、runtime fingerprint 和 execution provider evidence。
2. 把模型权重、external data、tokenizer、sampler、scheduler、vocoder、pre/post-process config、reduced runtime、Web runtime adapter 和 custom op sidecar 作为 package/VFS content entry，不使用 project-level `package_sections` 携带 payload。
3. 复用 `EncryptionDescriptor`、section hash、codec、migration、provider policy 和 package release gate；不设计模型专用容器、模型专用 DRM 或 loose sidecar 读取。
4. 固定 Shipping local AI 主 EP：Windows `DirectML`、Linux `OpenVINO`、macOS/iOS `CoreML`、Android `QNN`、Web `WebNN`；缺主 EP、operator coverage 不足、CPU fallback 或缺真实目标运行报告时阻断。
5. 将文本、图像和语音生成 chunk 写入 save extra section，正式 replay 读取 save payload；debug/live regeneration 只作为非权威差异报告。
6. 编写 ModelBundle manifest roundtrip、package/VFS lookup、encrypted model read failure/pass、vendor cache lock、custom op sidecar declaration、CPU fallback blocked、generated artifact save/replay 和 redaction 测试。

**Done Evidence:** `astra-ai-onnx` 不进入 EngineCore；provider 只通过 `McpAiSession` 和 package/VFS section ref 运行；Shipping local AI gate 能证明模型、runtime、EP、custom op、加密和生成结果 save/replay 都可审计。

**Linked Test IDs:** `T-S4-AI-ONNX`

## S4-AI-VFS-01 AI/MCP VFS evidence alignment

**ID:** `S4-AI-VFS-01`

**Status:** `REOPENED_SPEC`

**Goal:** AI/MCP 的 ModelBundle、Context Pack、generated artifact、tool access 和 package source 全部通过 Asset VFS mount evidence 表达。

**Depends On:** `S2-VFS-01`、`S4-AI-ONNX`、`S4-MCP-02`、[Asset VFS Blueprint](../../implementation/asset-vfs.md)

**Target Paths:** `Engine/Source/Developer/astra-ai/src/model_bundle.rs`、`Engine/Source/Developer/astra-mcp/src/context_pack.rs`、`Engine/Source/Developer/astra-release/tests/ai_mcp_gate.rs` planned target

**Steps:**

1. 让 `ai.model_bundle_manifest` 只引用 VFS locator、package section ref、hash、codec、license/provenance 和 execution provider evidence。
2. 禁止 Shipping provider 读取 loose model sidecar、runtime binary、tokenizer 或 custom op；开发期下载也必须进入 vendor cache 或 package-backed VFS。
3. Context Pack 的 read/search tool 只返回脱敏 `VfsUri`、hash、section ref 和 source span，不返回本地 root、provider secret 或 payload。
4. Generated artifact save section 记录 artifact section ref、chunk hash、validator status 和 replay policy，不重新请求 provider。
5. Release Gate 增加 `ai.model_bundle_vfs_mount`、`ai.context_pack_redaction` 和 package/source consistency check。

**Done Evidence:** `cargo test -p astra-ai onnx_model_bundle`、`cargo test -p astra-mcp context_tooling` 和 `cargo test -p astra-release ai_mcp_gate` 通过；release report 输出 ModelBundle VFS mount、Context Pack redaction 和 provider-free replay evidence。

**Linked Test IDs:** `T-S4-AI-VFS-01`

## S4-AI-03 Editor Copilot、Trusted session 与 Review Queue

**ID:** `S4-AI-03`

**Goal:** Editor Copilot 支持 Review Queue 和 Trusted session，所有写入生成 patch、audit event 和 undo checkpoint。

**Depends On:** `S4-EDITOR-05`、`S4-AI-02`

**Target Paths:** `Engine/Source/Developer/astra-ai/src/editor_copilot.rs`、`Engine/Source/Developer/astra-ai/src/trusted_session.rs`、`Engine/Tests/AI/editor_copilot.rs` planned target

**Steps:**

1. 定义 TrustedSessionScope，包含 project、path range、operation type 和 expiration。
2. 未授权写入进入 Review Queue，授权写入生成 patch 和 undo checkpoint。
3. 所有 AI 操作写 audit event，记录 provider profile、prompt hash、tool call 和 affected paths。
4. AI Control 显示 provider binding、MCP session、Context Pack 预览、Review Queue 和加密 trace 入口。
5. 编写 unauthorized queued、trusted write、undo checkpoint、audit event 和 AI Control render 测试。

**Done Evidence:** Copilot 不能绕过权限、审计和回滚。

**Linked Test IDs:** `T-S4-AI-03`

## S4-AI-04 Runtime memory and archive

**ID:** `S4-AI-04`

**Goal:** Runtime memory 管理角色设定、故事事实、episodic event、玩家偏好和自动压缩归档。

**Depends On:** `S4-AI-01`、`Docs/implementation/runtime-ai-director-memory.md`

**Target Paths:** `Engine/Source/Developer/astra-ai/src/runtime_memory.rs`、`Engine/Source/Developer/astra-ai/tests/runtime_memory.rs` planned target

**Steps:**

1. 定义 `MemoryEntry`、`MemoryAuthority`、`MemoryLayer`、`MemoryNamespace` 和 memory ledger。
2. `Canon` 默认只读；`Episodic` 和 `Player` 按创作者策略读写。
3. 实现 working、short-term、long-term、archive 分层和自动压缩归档。
4. Embedding/vector index 只作为可重建缓存，不参与 save 权威判断。
5. 编写 canon write denied、episodic append、player memory consent、compaction replay 和 index rebuild 测试。

**Done Evidence:** 角色记忆可回放，自动归档可审计，玩家 memory 的云端读取受首启同意约束。

**Linked Test IDs:** `T-S4-AI-04`

## S4-MCP-01 MCP tool descriptor 与 capability protocol

**ID:** `S4-MCP-01`

**Goal:** MCP tool 只能通过声明过的 project、Runtime、Editor、memory 和 Release Gate capability 访问能力。

**Depends On:** `S4-AI-02`、`Docs/contracts/ai-mcp.md`

**Target Paths:** `Engine/Source/Developer/astra-mcp/src/tool_descriptor.rs`、`Engine/Source/Developer/astra-mcp/src/capability.rs`、`Engine/Tests/MCP/capability_protocol.rs` planned target

**Steps:**

1. 定义 MCP tool descriptor、permission scope、audit fields 和 result schema。
2. 把 Runtime、Editor、Package、memory 和 Release Gate 操作映射到 capability check。
3. 拒绝未授权 path、operation、memory namespace 和 session。
4. 编写 capability allowed、denied、memory denied 和 audit propagation 测试。

**Done Evidence:** MCP 不能绕过 Editor/CLI 的 release gate 和 permission policy。

**Linked Test IDs:** `T-S4-MCP-01`

## S4-MCP-02 Context Pack and command allowlist

**ID:** `S4-MCP-02`

**Goal:** 外部 AI 工具通过 Bounded Context Pack、read/search tool 和命令白名单获得 Editor 等价能力。

**Depends On:** `S4-MCP-01`、`Docs/implementation/mcp-context-tooling.md`

**Target Paths:** `Engine/Source/Developer/astra-mcp/src/context_pack.rs`、`Engine/Source/Developer/astra-mcp/src/command_allowlist.rs`、`Engine/Tests/MCP/context_tooling.rs` planned target

**Steps:**

1. 定义 `ContextPack`、`context.read`、`context.search`、`memory.read` 和 `memory.search`。
2. Context Pack 默认脱敏，不返回本地绝对路径、provider secret、商业 payload 或 native handle。
3. `command.run` 只允许声明式 check、test、package 和 report 命令模板。
4. 发布运行时默认只启用内部 MCP session；外部 endpoint 需要 profile 和用户同意。
5. 编写 context read allowed、redaction blocked、command allowlist、runtime external endpoint opt-in 测试。

**Done Evidence:** 外部工具可以自动读写和运行检查，但不能拿到任意 shell 或无界上下文。

**Linked Test IDs:** `T-S4-MCP-02`

## S4-GATE-01 Provider-free replay 与 audit gate

**ID:** `S4-GATE-01`

**Goal:** Release Gate 覆盖 provider profile、Runtime Director、memory、MCP audit、debug trace、玩家同意和 provider-free replay。

**Depends On:** `S4-AI-01`、`S4-AI-02`、`S4-AI-ONNX`、`S4-AI-04`、`S4-MCP-02`

**Target Paths:** `Engine/Source/Developer/astra-release/src/ai_gate.rs`、`Engine/Source/Developer/astra-release/tests/ai_mcp_gate.rs` planned target

**Steps:**

1. 增加 `ai.provider_profile`、`ai.model_bundle`、`ai.onnx_runtime_pack`、`ai.onnx_execution_provider`、`ai.runtime_provider_startup`、`ai.generated_artifact_save`、`ai.provider_free_replay`、`ai.runtime_memory_policy`、`ai.debug_trace_redaction`、`ai.player_consent`、`mcp.context_permission` 和 `mcp.command_allowlist` gate check。
2. 校验 save/replay 中 committed AI output 完整，且 replay 不需要 provider。
3. 校验 Trusted session audit chain、Review Queue disposition、memory ledger 和 encrypted debug trace policy。
4. 编写 gate pass、missing audit blocked、provider startup blocked、ModelBundle blocked、CPU fallback blocked、generated artifact save blocked、memory policy blocked、context permission blocked 和 provider replay blocked 测试。

**Done Evidence:** v0.4 gate 能阻断缺失 audit、Live AI provider 不可用、memory 越权、MCP 越权或 provider-free replay 失败的 package。

**Linked Test IDs:** `T-S4-GATE-01`

## S4-EDITOR-RUNTIME-PROVIDER-01 Editor runtime provider switching

**ID:** `S4-EDITOR-RUNTIME-PROVIDER-01`

**Status:** `REOPENED_SPEC`

**Goal:** AstraEditor shell 通过 `ProductRuntimeProvider` 切换 Project Wizard、authoring surface、PIE、Debugger 和 Release Gate，不把 AstraVN 写成唯一玩法类型。

**Depends On:** `S3-RUNTIME-PROVIDER-01`、`S4-EDITOR-01`、`S4-PLUGIN-01`、[Game Runtime Provider Contract](../../contracts/game-runtime-provider.md)、[Editor Workflow Blueprint](../../implementation/editor-workflow.md)、[Editor Runtime Provider Migration](../../migrations/editor-runtime-provider-migration.md)

**Target Paths:** `Editor/Source/Bridge/astra-editor-bridge/src/runtime_provider.rs`、`Editor/Source/Bridge/astra-editor-bridge/src/pie.rs`、`Editor/Tests/runtime_provider_switch.rs` planned target

**Steps:**

1. Bridge 增加 `list_runtime_providers`、`read_runtime_editor_metadata` 和 `set_active_editor_target`。
2. Project Wizard 根据 `RuntimeEditorMetadata.project_templates` 显示 NativeVN 模板，并对 planned AstraEMU/AstraRPG provider 显示不可用诊断。
3. Panel registry 根据 `authoring_surfaces` 显示 `.astra` Script、VN Graph、Timeline、System UI、Luau policy，隐藏未绑定 runtime 的专属面板。
4. PIE 和 Release Gate request 必须携带 Game target id、provider id 和 profile；缺 binding 或 provider/profile mismatch 时 blocking。
5. 编写 provider selection、metadata redaction、PIE handoff、panel visibility 和 release gate handoff 测试。

**Done Evidence:** `cargo test -p astra-editor-bridge runtime_provider_switch` 通过；Editor report 只记录 provider id、surface id、source ref、hash 和 diagnostic，不记录 Editor widget、本地 root 或 payload。

**Linked Test IDs:** `T-S4-EDITOR-RUNTIME-PROVIDER-01`

## S4-EDITOR-TARGET-01 AstraEditor Editor target

**ID:** `S4-EDITOR-TARGET-01`

**Goal:** AstraEditor 以 `Editor` target 运行 Project Wizard、PIE、Debugger 和 Release Gate panel。

**Depends On:** `S1-TARGET-01`、`S4-EDITOR-01`、`S4-EDITOR-02`、`S4-EDITOR-05`

**Target Paths:** `Editor/Source/App/astra-editor/`、`Editor/Source/Bridge/astra-editor-bridge/src/target.rs`、`Editor/Tests/editor_target.rs` planned target

**Steps:**

1. 定义 `astra-editor` Target，kind 为 `editor`，绑定 desktop platforms，`packaged` 为 false。
2. Editor launch 前校验 Target manifest，不允许 Editor target 写入 packaged runtime。
3. PIE 使用所选 Game target 和 gameplay runtime provider 启动 RuntimeWorld，不复用 Editor target 的 state。
4. 编写 Editor target launch、PIE target handoff 和 package isolation 测试。

**Done Evidence:** Editor target 不出现在 game package，PIE 能显式选择 Game target。

**Linked Test IDs:** `T-S4-EDITOR-TARGET-01`

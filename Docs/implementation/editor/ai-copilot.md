# AstraEditor AI Copilot 与 MCP 设计

AI Copilot 和 MCP 的所有写入必须可审计、可回滚，并通过 provider-free replay。本文档描述 Editor Copilot 面板、Review Queue 五步确认 UX、inline ghost text、Trusted session、MCP capability 协议和 AI/MCP release gate 的前端设计。

Runtime AI Director（`McpAiSession` → `IntentValidator` → committed output → save/replay）属于 Engine 层，本文档仅涵盖 Editor 前端部分。完整 AI/MCP 架构见 [ai-mcp-runtime.md](../ai-mcp-runtime.md)。

---

## 1. AI 写入分级策略

所有 AI 写入按规模和影响范围分为两级：

| 级别 | 触发条件 | 审核流程 | 记录 |
| --- | --- | --- | --- |
| **inline hint**（一级） | ≤ 5 行，仅影响当前光标位置附近文本 | 用户 `Tab` 接受即直接写入 Script Editor 文字缓冲 | 写入 audit event（provider、prompt hash、output hash、行范围） |
| **patch**（二级） | > 5 行，或影响多个文件/面板，或由 Copilot 面板的 Generate 命令触发 | 必须进入 **Review Queue 五步确认** | 生成 patch + graph diff + audit event + undo checkpoint + release check provenance |

inline hint 接受后，Bridge 侧记录 audit event 到 `project_audit` sink（`astra-mcp` 的 audit sink 机制），不进入 Review Queue。

---

## 2. AI Copilot 面板

### 2.1 面板布局（混合：独立面板 + Script Editor inline）

```qml
// panels/AiCopilot.qml
Item {
    // ── 顶部：Session 控制 ─────────────────────────────────
    AiSessionHeader {
        providerName:   bridge.aiProviderName     // "OpenAI gpt-4o" / "Ollama llama3" 等
        sessionScope:   bridge.aiSessionScope     // 文件范围/项目范围
        trustedUntil:   bridge.trustedSessionStep // 0 = 未授权；>0 = 剩余 step 数
        onSwitchProvider: bridge.showProviderSettings()
        onRevokeTrusted:  bridge.revokeAllTrustedSessions()
    }

    // ── 中部：Multi-turn 对话历史 ──────────────────────────
    ListView {
        id: chatHistory
        model:    bridge.chatHistoryModel   // QAbstractListModel
        delegate: AiChatBubble {
            role:       model.role    // "user" | "assistant" | "system"
            contentMd:  model.content // Markdown 渲染
            auditRef:   model.auditRef  // 点击查看 audit event
            hasReview:  model.hasPendingReview
            onReviewClicked: reviewQueue.revealItem(model.reviewItemId)
        }
    }

    // ── 底部：输入框 ───────────────────────────────────────
    AiInputBar {
        inputHint: qsTr("向 Copilot 描述你想做什么...")
        contextFiles: bridge.aiContextFiles   // 当前 AI session 的 Context Pack 文件范围
        onSubmit: function(text) {
            bridge.submitCopilotRequest(text)
        }
        onAddContext: bridge.showContextPicker()
    }
}
```

### 2.2 inline Ghost Text（Script Editor 内嵌）

在 `ScriptEditor.qml` 内，ghost text 以灰色半透明文字显示在光标后方：

```qml
// panels/ScriptEditor.qml 内的 GhostTextOverlay
Item {
    id: ghostTextOverlay
    visible: bridge.hasGhostText && scriptEditorRoot.activeFocus

    // Ghost text 渲染：用 QQuickPaintedItem 或 Text 叠加
    Text {
        text:  bridge.ghostTextContent
        color: tokens.textDisabled
        font:  textArea.font
        // 位置对齐到光标所在行末尾
        x: textArea.cursorRectangle.x
        y: textArea.cursorRectangle.y
    }

    // 键盘事件：Tab 接受，Escape 拒绝
    Keys.onPressed: function(event) {
        if (event.key === Qt.Key_Tab) {
            bridge.acceptGhostText()
            event.accepted = true
        } else if (event.key === Qt.Key_Escape) {
            bridge.dismissGhostText()
            event.accepted = true
        }
        // Ctrl+→ 逐词接受（Stage 5）
    }
}
```

Bridge 侧 ghost text 触发逻辑：

```rust
// 停止输入 800ms 后，若上下文适合，向 AI provider 请求 inline hint
pub async fn request_ghost_text(&mut self, cursor_context: CursorContext) {
    let hint = self.ai_session.request_inline_hint(cursor_context).await?;
    if hint.line_count <= 5 {
        // 直接在 QML 侧展示 ghost text
        self.emit_ghost_text(hint);
    } else {
        // 超出 5 行，转入 Review Queue
        self.create_review_item(hint.into_patch()).await?;
    }
}
```

---

## 3. Review Queue 五步确认 UX

Review Queue 是一个常驻侧边面板（类 GitHub PR Review）。每次二级 AI 写入产生一个 `ReviewItem`，包含五个确认步骤：

```qml
// components/ReviewQueue.qml
Item {
    // 未处理的 ReviewItem 列表（红色角标数字提示）
    ReviewQueueHeader {
        pendingCount: bridge.reviewQueueCount
    }

    ListView {
        model:    bridge.reviewQueueModel
        delegate: ReviewItem {
            id:           model.reviewItemId
            title:        model.title        // "AI: 生成对话段落 (第 42-67 行)"
            provider:     model.providerName
            promptHash:   model.promptHash

            // ── 五步 Tab ──────────────────────────────────
            TabBar {
                id: reviewTabs
                TabButton { text: qsTr("差异 (Diff)");       icon.name: "diff" }
                TabButton { text: qsTr("图变更 (Graph)");    icon.name: "graph-diff" }
                TabButton { text: qsTr("审计事件 (Audit)");  icon.name: "audit" }
                TabButton { text: qsTr("撤销点 (Undo)");     icon.name: "undo" }
                TabButton { text: qsTr("发布检查 (Gate)");   icon.name: "gate" }
            }
            StackLayout {
                currentIndex: reviewTabs.currentIndex

                // Step 1：Diff 视图
                DiffViewer {
                    diffJson: model.patchJson
                    // 高亮新增（绿色）、删除（红色）行
                }

                // Step 2：Graph 差异视图
                GraphDiffViewer {
                    diffJson: model.graphDiffJson
                    // 显示新增/修改/删除的节点和连线
                }

                // Step 3：审计事件
                AuditEventViewer {
                    auditJson: model.auditEventJson
                    // 显示：provider profile、prompt hash、tool call 列表、affected paths
                }

                // Step 4：撤销检查点
                UndoCheckpointViewer {
                    checkpointJson: model.undoCheckpointJson
                    // 显示：patch id、source map baseline、撤销命令
                }

                // Step 5：发布检查
                ReleaseCheckViewer {
                    checksJson: model.releaseChecksJson
                    // 显示：每个 release check id 的 pass/blocked/warning 状态
                }
            }

            // ── 操作按钮 ──────────────────────────────────
            Row {
                Button {
                    text:      qsTr("应用 (Apply)")
                    enabled:   model.releaseChecksAllPass  // 全部 gate check 通过才能 Apply
                    onClicked: bridge.applyReviewItem(model.reviewItemId)
                }
                Button {
                    text:    qsTr("拒绝 (Reject)")
                    onClicked: {
                        rejectReasonDialog.open()
                        rejectReasonDialog.onAccepted.connect(function(reason) {
                            bridge.rejectReviewItem(model.reviewItemId, reason)
                        })
                    }
                }
                // 强制应用（即使 gate check warning，需要二次确认）
                Button {
                    text:    qsTr("强制应用...")
                    visible: model.hasGateWarnings && !model.hasGateBlocking
                    onClicked: forceApplyConfirmDialog.open()
                }
            }
        }
    }
}
```

### 3.1 Apply 流程

```
用户点击 Apply
  → Bridge.apply_review_item(id)
  → 验证 release checks 全部 pass（blocking 则拒绝）
  → 应用 source patch 到 .astra（或 Luau policy override）
  → 触发 compile → identity check
  → 写入全局 undo checkpoint（可 Ctrl+Shift+Z 回滚）
  → 写入 audit event（Trusted Applied 状态）
  → 从 Review Queue 移除
  → signal: reviewQueueCount 更新
```

### 3.2 Reject 流程

```
用户点击 Reject
  → 输入拒绝原因（可选）
  → 写入 audit event（Rejected 状态 + 原因）
  → 从 Review Queue 移除（patch 不应用）
  → undo checkpoint 不写入（无需 undo）
```

---

## 4. Trusted Session

Trusted session 允许 AI 在特定路径范围内直接写入（不进 Review Queue），但仍生成 patch、audit event 和 undo checkpoint。

```rust
pub struct TrustedSessionScope {
    pub project:      ProjectId,
    pub path_roots:   Vec<ProjectPath>,  // 允许直接写入的路径范围
    pub operations:   Vec<EditorOperationKind>,  // 允许的操作类型
    pub expires_at_step: u64,            // 在哪个 global patch step 后过期
}
```

QML 侧显示 Trusted session 状态：

```qml
// AiCopilot.qml 中的 TrustedSessionBadge
Rectangle {
    visible: bridge.trustedSessionStep > 0
    color: "#1e3a1e"  // 深绿色背景
    radius: 4
    Row {
        Image { source: "qrc:/icons/shield-check.svg" }
        Text {
            text: qsTr("已授权（剩余 %1 步）").arg(bridge.trustedSessionStep)
            color: tokens.textSuccess
        }
        Button {
            text: qsTr("撤销")
            onClicked: bridge.revokeAllTrustedSessions()
        }
    }
}
```

---

## 5. Context Pack 预览

AI session 的 Context Pack（`McpAiSessionScope.allowed_context_roots`）在 Copilot 面板底部以文件 chip 列表展示：

```qml
// AiCopilot.qml 中的 ContextPackPreview
Flow {
    Repeater {
        model: bridge.contextPackFiles   // 脱敏 VfsUri 列表
        delegate: ContextFileChip {
            filename: model.displayName  // 显示相对路径，不显示绝对路径
            onRemove: bridge.removeContextFile(model.vfsUri)
        }
    }
    Button {
        text: qsTr("+ 添加文件")
        onClicked: bridge.showContextPicker()
    }
}
```

Context Pack 读取工具（`context.read`、`context.search`）只返回脱敏 `VfsUri`、hash、section ref 和 source span，不返回本地绝对路径、provider secret 或商业 payload。

---

## 6. AI Provider 配置面板

AI provider 默认禁用，需要显式绑定。`ProjectSettings.qml` 中的 AI 配置区：

```qml
// panels/ProjectSettings.qml 中的 AIProviderSection
GroupBox {
    title: qsTr("AI Provider")
    Column {
        // Provider 选择下拉
        ComboBox {
            model:          bridge.availableAiProviders  // ["None", "OpenAI", "Ollama", "ONNX Runtime"]
            currentIndex:   bridge.activeAiProviderIndex
            onActivated:    bridge.setAiProvider(currentText)
        }

        // OpenAI 配置（仅 provider = OpenAI 时显示）
        Loader {
            active: bridge.activeAiProvider === "OpenAI"
            sourceComponent: OpenAiConfig {
                apiKeyStored: bridge.openAiKeyStored  // 只显示「已存储」，不显示明文
                onSetKey: bridge.setOpenAiKey        // 调用 AstraPlatform 提供的平台 SecretProvider 存储
                model:    bridge.openAiModel         // "gpt-4o" / "gpt-4o-mini" 等
                onModelChanged: bridge.setOpenAiModel
            }
        }

        // Ollama 配置（本地端点）
        Loader {
            active: bridge.activeAiProvider === "Ollama"
            sourceComponent: OllamaConfig {
                endpoint:    bridge.ollamaEndpoint   // "http://localhost:11434"
                model:       bridge.ollamaModel
                onEndpointChanged: bridge.setOllamaEndpoint
            }
        }

        // ONNX Runtime（packaged 本地 AI）
        Loader {
            active: bridge.activeAiProvider === "ONNX Runtime"
            sourceComponent: OnnxConfig {
                modelBundleVfsUri: bridge.onnxModelBundleUri
                onBrowse:          bridge.browseOnnxModelBundle()
                executionProvider: bridge.onnxExecutionProvider  // "DirectML" / "CoreML" 等
                epStatus:          bridge.onnxEpStatus           // "available" / "missing"
            }
        }

        // Data egress 声明（cloud AI 时显示）
        Loader {
            active: bridge.aiHasCloudEgress
            sourceComponent: DataEgressWarning {
                // 说明哪些数据会发送到云端，让用户知情
            }
        }
    }
}
```

---

## 7. MCP Tool 描述符（Editor 前端部分）

MCP tool 在 Editor 侧通过 `astra-mcp` 的 `ToolDescriptor` 注册。Editor 的 Plugin Manager 显示已注册的 MCP tool 列表：

```qml
// panels/PluginManager.qml 中的 McpToolSection
ListView {
    model: bridge.mcpToolModel
    delegate: McpToolRow {
        toolId:        model.toolId
        mutating:      model.mutating
        requiredScope: model.requiredSession
        permissions:   model.permissions
        auditable:     model.auditSink !== ""
        rollbackPolicy: model.rollback
        packaged:      model.packaged   // false = Editor-only
    }
}
```

---

## 8. AI/MCP Release Gate 面板集成

`ReleaseGatePanel.qml` 包含 AI/MCP gate checks（对应 S4-GATE-01 工作项）。当 AI/MCP gate check 失败时，面板显示：

| Check ID | 失败说明 |
| --- | --- |
| `ai.provider_profile` | AI provider profile 声明缺失或不完整 |
| `ai.model_bundle` | ONNX ModelBundle VFS mount 路径无效 |
| `ai.onnx_execution_provider` | 目标平台缺少主 EP（DirectML/CoreML 等） |
| `ai.provider_free_replay` | save/replay 中存在需要 provider 的 AI output，无法 provider-free 回放 |
| `ai.runtime_memory_policy` | 角色记忆 cloud 读取缺少玩家同意声明 |
| `ai.debug_trace_redaction` | release package 中含明文 prompt 或 Context Pack |
| `ai.player_consent` | 联网 AI feature 缺少玩家首启同意 |
| `mcp.context_permission` | MCP tool 访问了未在 Context Pack 中声明的路径 |
| `mcp.command_allowlist` | MCP `command.run` 使用了白名单外的命令 |

每个失败 check 显示「跳转到 audit log」和「查看诊断详情」链接。

---

## 9. Save/Replay Inspector（AI 部分）

Save/Replay Inspector（`SaveReplayInspector.qml`）中的 AI 相关 section：

| Section | 说明 |
| --- | --- |
| `committed_ai_output` | 已提交的 AI 输出（provider、model fingerprint、output hash） |
| `ai_memory_ledger` | 角色记忆层级（Canon/Episodic/Player）快照 |
| `ai_generated_artifact` | ONNX 生成的文本/图像/语音 chunk section ref |

Save/Replay Inspector Stage 4 实现：section 结构浏览 + PIE 内从特定 save section seek。Replay hash 验证和 save diff 推迟到 Stage 5。

---

## 10. 验收标准

```bash
# AI/MCP 测试（S4-AI-* 和 S4-MCP-* 工作项）
cargo test -p astra-ai runtime_ai_replay
cargo test -p astra-ai editor_copilot
cargo test -p astra-ai runtime_memory
cargo test -p astra-mcp capability_protocol
cargo test -p astra-mcp context_tooling
cargo test -p astra-release ai_mcp_gate
```

| 测试 | 验收条件 |
| --- | --- |
| `inline_hint_accepted` | ≤ 5 行 ghost text → Tab 接受 → audit event 写入，不进 Review Queue |
| `large_patch_review_queue` | > 5 行生成 → Review Queue 显示五步确认 → 五步全部可见 |
| `review_apply` | Apply → source patch 应用 → compile 通过 → undo checkpoint 可回滚 |
| `review_reject` | Reject + 原因 → audit event 写入「Rejected」→ patch 不应用 |
| `trusted_write` | Trusted session 授权 → AI 直接写入 → patch + audit event 生成（无 Review Queue） |
| `trusted_revoke` | 撤销 Trusted session → 后续写入回到 Review Queue |
| `gate_provider_free_replay` | replay 不需要 provider → gate check pass |
| `gate_missing_audit` | audit chain 缺失 → gate blocking |
| `gate_cloud_egress_no_consent` | 联网 AI 无玩家同意 → gate blocking |
| `context_pack_redaction` | Context Pack 读取结果不含绝对路径或 provider secret |

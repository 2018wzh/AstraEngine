# MCP / Agent 能力协议层设计

## 1. 定位

MCP 是 AstraEngine 的 Agent 能力协议层，负责 resources、tools、prompts、sessions、
permissions 和 audit。MCP 不等价于 AI Provider，不等价于 Runtime Director，也不直接生成内容。

Astra 使用两个 Host：

- `Editor MCP Host`：创作期工具与 AI 工作流。
- `Runtime MCP Host`：运行时受控反馈生成。

两者共享 session/audit 基础协议，但 resources、tools、permissions 和 release policy 分离。

## 2. Host 分层

```text
External Agent / Editor AI
  -> AstraMCPServer
  -> Editor MCP Host
  -> Editor / Developer Tools / Review Queue
  -> Text Source / Asset Draft / Validation / Cook / Package

Runtime Feedback
  -> Runtime MCP Host
  -> Runtime Context Builder
  -> RuntimeGenerationOrchestrator
  -> AI Provider
  -> AIIntent
  -> Validator / Director / ControlPolicy
  -> RuntimeEvent
  -> Save / Replay
```

## 3. Sessions

Editor sessions：

- `read_only`：读取 resources、生成 suggestion、生成 draft、不能写 canonical source。
- `review_write`：可把 proposal/draft 放入 Review Queue。
- `trusted_write`：可应用 patch、导入 draft、执行 mutating tool，必须写 Operation Log。

Runtime sessions：

- `runtime_read`：读取 runtime-safe context。
- `runtime_generate`：请求 intent/content generation。
- `runtime_commit`：提交已验证 committed output。
- `runtime_fallback`：选择 deterministic fallback。

Session 必须绑定 caller、project/runtime session、policy snapshot、capability set、release mode 和 audit sink。
Session 权限不能超过模块声明权限。

## 4. Editor MCP Host

Editor MCP Host 服务两条工作流：

- Editor Copilot MCP：建议、解释、patch proposal、测试/cook/release gate 辅助。
- Editor Content Generation MCP：资产、文本、音频、语音、视频、动画、filter/timeline draft 生成/修改/增强。

### Editor Resources

```text
astra://project/manifest
astra://project/config
astra://project/templates
astra://assets/registry
astra://assets/drafts
astra://assets/{id}
astra://scripts/{path}
astra://graphs/{id}
astra://timelines/{id}
astra://filters/{id}
astra://lore/{id}
astra://review-queue
astra://audit
astra://diagnostics
astra://release-gate/report
```

### Editor Tools

Copilot tools：

- `project.inspect`
- `project.plan_patch`
- `script.suggest`
- `graph.suggest`
- `timeline.suggest`
- `filtergraph.suggest`
- `diagnostics.explain`
- `schema.fix_proposal`
- `test.run_headless`
- `build.cook`
- `release.run_gate`

Content generation tools：

- `asset.generate_draft`
- `asset.modify_draft`
- `asset.enhance_draft`
- `asset.preview_draft`
- `asset.compare_variants`
- `asset.import_draft`
- `asset.validate`

Review/trusted tools：

- `review.enqueue`
- `review.apply`
- `project.apply_patch`
- `project.write_file`

Mutating tools only run in `trusted_write` or explicitly permitted review session.
They must write Operation Log and preserve rollback metadata.

## 5. Runtime MCP Host

Runtime MCP Host is packaged only in release modes that explicitly allow runtime generation.
It cannot write project files or canonical source.

### Runtime Resources

```text
astra://runtime/session
astra://runtime/world
astra://runtime/scene
astra://runtime/actors/{id}
astra://runtime/blackboards/{scope}
astra://runtime/director
astra://runtime/control-policy/{actorId}
astra://runtime/constraints
astra://runtime/feedback
astra://runtime/fallbacks
astra://runtime/committed-ai-output
astra://runtime/save-preview
```

### Runtime Tools

- `runtime.context.inspect`
- `runtime.feedback.submit`
- `runtime.intent.request`
- `runtime.intent.validate`
- `runtime.intent.commit`
- `runtime.fallback.select`
- `runtime.audit.annotate`

Runtime constraints：

- 不允许 `project_write`。
- 不允许直接写 AssetRegistry 或 Content。
- 不允许 provider raw output 绕过 IntentValidator。
- 不允许绕过 Save/Replay、Fallback、ControlPolicy 或 Director。
- Committed output 必须进入 save/replay。

## 6. Prompts

Prompts 只定义任务模板，不绑定 Provider。

Editor prompts：

- script rewrite
- graph cleanup
- timeline pacing
- asset metadata completion
- release gate explanation
- localization review
- draft generation brief

Runtime prompts：

- dialogue response
- ambient reaction
- character hint
- fallback narration
- constrained variation

Prompt 必须声明 required resources、allowed tools、safety policy、audit kind 和 output schema。

## 7. Provider And Tool Registration

MCP tools 可由内置 host 或插件注册：

- `IMcpToolProvider` 提供 resources/tools/prompts descriptor。
- `IAIProvider` 只负责模型能力，不直接注册 mutating tools。
- Provider 和 MCP tool 都必须声明 capability、permission、release eligibility。

Release Gate 检查：

- Editor-only MCP tool 不进入 packaged runtime。
- Runtime MCP tool 只在 Hybrid/Experimental build 中进入 package。
- Mutating editor tools 不能在 read-only session 中使用。
- Runtime tools 不能声明 project write。

## 8. Audit

- Editor mutating tools 写 Operation Log。
- Editor generation 写 Generation Audit Log。
- Runtime generation 写 Generation Audit Log 和 committed output reference。
- 同一次调用既生成又写入时，两类日志都写。

MCP 工具不得返回明文 API key、未授权文件内容、Editor UI object、C++ pointer 或 native handle。

## 9. 验收

- Editor Copilot 可生成 patch proposal，但 read-only session 不能写文件。
- Editor Content Generation 可生成 draft，只有 review/trusted session 可导入。
- Runtime MCP 可响应 feedback、生成 intent、验证、commit、保存、回放。
- Deterministic build 不包含 Runtime MCP Host。
- Hybrid build 包含 Runtime MCP Host 时，release gate 校验 provider、tool、audit、fallback policy。



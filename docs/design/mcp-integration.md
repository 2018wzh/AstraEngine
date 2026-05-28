# 统一 MCP / Agent 能力层设计

## 1. 目标

统一重构后，MCP 不再只表示 Editor/Developer 的附属开发接口，而是 AstraEngine 的统一 Agent 能力协议层。

它的职责是：

- 定义统一的 resources、tools、prompts、sessions 和权限边界。
- 让开发阶段 Agent 协作和运行时受控内容生成复用同一套协议表面。
- 保持 Editor/Game 分离，不把编辑器 trusted write 语义带入 runtime。
- 让 Runtime Generation、Provider 和 Audit 都能作为独立模块接入。

MCP 的非职责：

- 不直接生成内容。
- 不直接等价于 Runtime Generation Orchestrator。
- 不直接等价于 Provider。
- 不绕过 Boundary Manager、Save/Replay/Fallback 或 Release Gate。

## 2. 系统位置

```text
External Agent
  -> AstraMCPServer
  -> Editor MCP Host
  -> Editor / Developer Services
  -> Project Text Sources / Validation / Cook / Package

Runtime Session / Embedded Agent
  -> Runtime MCP Host
  -> Runtime Generation Orchestrator
  -> Runtime Services / RuntimeCommand
  -> Provider Modules
  -> Agent Audit
```

推荐模块：

```text
Engine
├── Runtime
│   ├── MCPCore
│   ├── RuntimeMCPHost
│   ├── RuntimeGeneration
│   └── AgentAudit
├── Editor
│   └── EditorMCPHost
├── Developer
│   └── MCPTools
└── Programs
    └── AstraMCPServer
```

## 3. Host 分层

### 3.1 Editor MCP Host

定位：

- 开发阶段 Agent 协作入口。
- 面向项目上下文、验证、构建、发布和 trusted direct write。
- 默认不进入 packaged runtime。

能力：

- 读取 Text-First project resources。
- 调用 Editor / Developer toolchain。
- 在 trusted session 中直接写 workspace/project 内文本源文件。
- 为 Agent Workbench、自动化脚本和外部 Agent 提供统一协议。

限制：

- 只允许访问当前 workspace/project。
- 不暴露明文密钥、Editor UI 对象、EnTT/ECS 内部状态。
- direct write 只限文本源文件，不允许把 cooked/package 产物当作 canonical source。

### 3.2 Runtime MCP Host

定位：

- 运行时受控内容生成入口。
- 面向 runtime-safe resources、tools、prompts 和会话。
- 只有在项目策略和打包资格允许时才可进入 packaged runtime。

能力：

- 读取运行时上下文、角色状态、剧情节点、受控 lore 视图和 Save/Replay 所需快照信息。
- 调用 runtime-safe tools，例如查询上下文、请求生成、选择 fallback、读取约束或记录运行时决策。
- 作为 Runtime Generation Orchestrator 的统一协议入口。

限制：

- 不允许 `project_write`。
- 不允许访问未授权外部路径。
- 不允许绕过 Save/Replay/Fallback。
- 不允许获得 Editor trusted session 语义。

## 4. Shared Session 模型

统一 MCP 层共享 session 概念，但不同 host 的 session 权限不同。

### 4.1 Editor Session

- `read_only`：只读项目资源和诊断。
- `trusted_write`：允许 mutating tools 修改文本源文件。

### 4.2 Runtime Session

- `runtime_read`：读取运行时上下文和受控状态。
- `runtime_generate`：允许触发 Runtime Generation Orchestrator。
- `runtime_fallback`：允许在既定策略内切换 fallback 或禁用动态生成。

Session 规则：

- Session 必须绑定 caller、project、policy snapshot 和 capability set。
- Session 权限来自项目策略、模块权限、构建模式和当前运行环境的交集。
- Session 不能提升模块未声明的权限。

## 5. Shared Resources / Tools / Prompts

### 5.1 Resources

MCP resources 暴露只读上下文，返回文本源数据或稳定 DTO。

Editor 侧示例：

```text
astra://project/manifest
astra://project/config
astra://assets/registry
astra://scripts/{path}
astra://story/graph/{id}
astra://lore/{id}
astra://review-queue
astra://build/status
```

Runtime 侧示例：

```text
astra://runtime/session
astra://runtime/scene
astra://runtime/characters/{id}
astra://runtime/story/current
astra://runtime/constraints
astra://runtime/save-preview
astra://runtime/fallbacks
```

规则：

- 不返回 editor widget、native pointer、ECS entity。
- 不返回明文密钥。
- 大对象支持分页或范围读取。
- 路径边界始终受 host 和 session 约束。

### 5.2 Tools

Editor 侧工具面：

- `project.inspect`
- `project.write_file`
- `asset.write_sidecar`
- `script.write`
- `story.write_graph`
- `review.enqueue`
- `test.run_headless`
- `build.cook`
- `build.package`
- `release.run_gate`

Runtime 侧工具面：

- `runtime.context.inspect`
- `runtime.constraints.inspect`
- `runtime.generation.request`
- `runtime.generation.cancel`
- `runtime.fallback.select`
- `runtime.audit.annotate`

Tool 规则：

- Mutating tools 必须写 Operation Log。
- Runtime mutating tools 只能修改运行时会话状态或受控生成状态，不能修改 project source。
- 动态注册工具必须声明 capability、permission、input schema、output schema 和 audit behavior。

### 5.3 Prompts

Prompts 是项目感知但 provider 无关的模板，Editor 和 Runtime 共享命名体系。

示例：

```text
prompt.dialogue_polish
prompt.lore_consistency_check
prompt.asset_draft
prompt.runtime_flavor_reply
prompt.runtime_reactive_response
prompt.runtime_branch_guarded
```

## 6. 审计模型

审计从 MCP 和生成逻辑中抽离，统一交给 `Agent Audit` 模块。

### 6.1 Operation Log

记录 tool side effect，例如：

- Editor MCP trusted direct write。
- runtime-safe tool 调用。
- 验证/构建/打包动作。

建议路径：

```text
Saved/Agent/Audit/Operations/*.operation.yaml
```

### 6.2 Generation Audit Log

记录内容生成来源，例如：

- prompt hash
- context hash
- output hash
- provider id
- fallback path
- session id
- final RuntimeCommand / reviewed patch id

建议路径：

```text
Saved/Agent/Audit/Generation/*.generation.yaml
```

规则：

- 两类日志都追加写入。
- Editor trusted direct write 至少写 Operation Log。
- Runtime generation 至少写 Generation Audit Log。
- 如果某次写入既有工具副作用又有 AI 生成来源，两种日志都写。

## 7. 与 Runtime Generation / Provider 的关系

职责划分必须保持清晰：

- MCP：统一协议层，负责 resources / tools / prompts / sessions / permissions。
- Runtime Generation Orchestrator：负责 Context Builder、Boundary Manager、fallback、回放和把输出落成 RuntimeCommand。
- Provider Modules：负责模型/服务调用，例如云端 LLM、本地 LLM、图像生成、TTS。
- Agent Audit：负责记录副作用和来源，不负责执行生成。

这意味着“统一 MCP 化”是统一协议和会话模型，不是把生成、provider 和审计都塞进同一个模块。

## 8. 打包与安全

默认策略：

- Editor MCP Host 默认禁用。
- Packaged runtime 默认不包含 Editor MCP Host。
- Runtime MCP Host 默认不打包；仅在项目策略显式开启运行时生成，且模块声明 `runtime.packaged` 后才可进入 packaged runtime。
- Provider Modules 必须独立声明 `network`、`ai.provider` 和 `runtime.packaged` 等权限。
- Agent Audit 可以进入 packaged runtime，但必须遵守隐私和日志裁剪策略。

安全规则：

- MCP 不暴露明文 API key。
- Runtime MCP Host 不暴露宿主文件系统任意访问。
- Editor MCP trusted direct write 必须限制在 workspace/project 内文本源文件。
- Runtime MCP Host 不得绕过 Save/Replay/Fallback。
- Release Gate 必须校验 runtime MCP、generation、provider 和 audit 模块的 packaged eligibility。

## 9. 测试

最低测试：

- Editor session path boundary test。
- Editor trusted direct write operation log test。
- Runtime session capability denial test。
- Runtime generation request -> RuntimeCommand playback test。
- Provider permission and packaged eligibility test。
- Agent Audit operation/generation split log test。
- Secret redaction test。
- Dynamic MCP host/tool/resource registration test。

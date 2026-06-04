# MCP / Agent 能力协议层设计

## 1. 定位

MCP 是 AstraEngine 的 Agent 能力协议层，负责 resources、tools、prompts、sessions 和权限边界。MCP 不等价于 AI Provider，不等价于 Runtime Director，也不直接生成内容。

## 2. Host 分层

```text
External Agent
  -> AstraMCPServer
  -> Editor MCP Host
  -> Editor / Developer Tools
  -> Text Source / Validation / Cook / Package

Runtime Session
  -> Runtime MCP Host
  -> Runtime Generation Orchestrator
  -> AI Provider
  -> AIIntent
  -> Validator / Director
  -> RuntimeEvent
```

### Editor MCP Host

用于开发阶段：

- 读取项目文本源数据、资产、脚本、剧情图、设定和审计。
- 调用验证、测试、Cook、Package、Release Gate。
- 在 trusted session 中写 workspace/project 内文本源文件。

限制：

- 默认禁用，需要显式 trusted session。
- 不进入 packaged runtime。
- 不暴露明文密钥、Editor widget、native pointer。

### Runtime MCP Host

用于运行时受控生成：

- 读取 runtime-safe context、角色状态、剧情阶段、约束和 fallback。
- 请求 Runtime Generation Orchestrator 生成 Intent。
- 记录 runtime audit。

限制：

- 不允许 project_write。
- 不允许未授权外部路径访问。
- 不允许绕过 Save/Replay/Fallback、ControlPolicy 或 Director。

## 3. Shared Session

Editor session：

- `read_only`
- `trusted_write`

Runtime session：

- `runtime_read`
- `runtime_generate`
- `runtime_fallback`

Session 必须绑定 caller、project、policy snapshot、capability set 和 audit sink。Session 权限不能超过模块声明权限。

## 4. Resources / Tools / Prompts

Editor resources：

```text
astra://project/manifest
astra://project/config
astra://assets/registry
astra://scripts/{path}
astra://story/{id}
astra://lore/{id}
astra://review-queue
astra://audit
```

Runtime resources：

```text
astra://runtime/session
astra://runtime/scene
astra://runtime/actors/{id}
astra://runtime/constraints
astra://runtime/fallbacks
astra://runtime/save-preview
```

Editor tools：

- `project.inspect`
- `project.write_file`
- `asset.validate`
- `script.validate`
- `review.enqueue`
- `test.run_headless`
- `build.cook`
- `release.run_gate`

Runtime tools：

- `runtime.context.inspect`
- `runtime.intent.request`
- `runtime.fallback.select`
- `runtime.audit.annotate`

Prompts 只定义任务模板，不绑定 Provider。

## 5. 审计

Mutating tools 写 Operation Log。Runtime generation 写 Generation Audit Log。如果一次工具调用既产生副作用又生成内容，两类日志都写。

MCP 工具不得返回明文 API key、未授权文件内容、Editor UI 对象或内部 native handle。

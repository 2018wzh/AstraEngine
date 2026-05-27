# MCP Integration 设计

## 1. 目标

MCP Integration 让外部 Agent 和工具通过标准协议访问 AstraEngine 的编辑器和开发工具链。

MCP 的定位：

- Editor / Developer 插件能力。
- 本地受信开发接口。
- 完整工具链入口。
- 不替代 AI Provider。
- 不进入默认 packaged runtime。

MCP 的核心原则：

- 外部 Agent 通过 MCP 访问稳定 resources、tools、prompts。
- MCP 不暴露 EnTT、ECS entity、编辑器 UI 对象或内部指针。
- MCP mutating tools 在 trusted session 中允许直接写入项目源文件。
- 每次 mutating tool 调用必须写入 Operation Log。
- MCP resources、tools 和 prompts 可以由动态模块通过 ExtensionRegistry 注册，但模块必须声明 `mcp.register_tools` 等权限。

## 2. 系统位置

```text
External Agent / MCP Host
  -> AstraMCPServer
  -> MCP Tool Router
  -> Editor / Developer Services
  -> Project Text Sources / Build Tools / Validation Tools
```

推荐模块：

```text
Engine
├── Editor
│   └── MCPIntegration
├── Developer
│   └── MCPTools
├── Runtime
│   └── ExtensionRegistry
└── Programs
    └── AstraMCPServer
```

第一阶段可以只作为 Editor/Developer 插件实现；`AstraMCPServer` 可作为后续独立开发工具封装同一套服务。

## 3. Hosting

### 3.1 默认策略

- MCP server 默认禁用。
- 用户必须显式为当前项目启动 trusted session。
- MCP server 默认只服务当前 workspace/project。
- Packaged runtime 默认不包含 MCP server。

### 3.2 Transport

第一阶段 transport：

- `stdio`。

后续可扩展：

- 本地 HTTP。
- WebSocket。
- Editor 内嵌会话。

## 4. Trusted Direct Write

MCP 受信会话允许直接写入 workspace/project 内文本源文件。

这是一条明确例外：

- 普通 AI 输出默认进入 Review Queue。
- MCP trusted direct write 不强制进入 Review Queue。
- Review Queue 仍可作为可选 MCP tool 使用。

限制：

- 不允许访问明文 API key。
- 不允许访问未授权外部路径。
- 不允许暴露或写入 EnTT/ECS 内部状态。
- 不允许修改 packaged runtime 产物作为源数据。
- 不允许把 DerivedDataCache 当作 canonical source。
- 不允许未声明权限的动态模块注册 mutating MCP tool。

每次 mutating tool 必须记录：

```yaml
operation_id: mcp_20260527_0001
session_id: session_abc123
tool: script.write
input_hash: sha256_input_without_secrets
affected_paths:
  - Content/Scripts/chapter_01.astra
timestamp: 2026-05-27T00:00:00Z
caller: local_trusted_agent
before_hash: sha256_before
after_hash: sha256_after
validation:
  script_validate: pass
  release_gate_required: false
result: success
diagnostics: []
```

Operation Log 建议路径：

```text
Saved/MCP/OperationLog/*.mcp-operation.yaml
```

## 5. Resources

MCP resources 暴露只读上下文。

```text
astra://project/manifest
astra://project/config
astra://assets/registry
astra://assets/{asset_id}/metadata
astra://scripts/{path}
astra://story/graph/{id}
astra://lore/{id}
astra://characters/{id}
astra://localization/{locale}
astra://review-queue
astra://audit-log
astra://build/status
```

Resource 规则：

- 返回文本源数据或稳定 DTO。
- 不返回 editor widget、native pointer、ECS entity。
- 不返回密钥。
- 对大文件支持分页或范围读取。
- 路径必须限制在当前 workspace/project。

## 6. Tools

第一阶段工具面覆盖完整开发链路。

### 6.1 Project

- `project.open`
- `project.inspect`
- `project.write_file`

### 6.2 Assets

- `asset.query`
- `asset.write_sidecar`
- `asset.validate_sidecars`

### 6.3 Script / Story

- `script.validate`
- `script.write`
- `story.validate_graph`
- `story.write_graph`

### 6.4 Authoring Data

- `lore.write`
- `character.write`
- `localization.write`

### 6.5 Review / Audit

- `review.enqueue`
- `audit.generate_ai_report`

### 6.6 Test / Build / Release

- `test.run_headless`
- `build.cook`
- `build.package`
- `release.run_gate`

### 6.7 Compatibility

- `compat.probe_project`
- `compat.validate_mount`
- `compat.inspect_assets`
- `compat.inspect_scripts`
- `compat.validate_modernization`
- `compat.generate_diagnostics`

Tool 规则：

- Mutating tools write Operation Log.
- Validation tools return structured diagnostics.
- Build tools return artifact paths and summary status.
- Compatibility tools write diagnostics, module config, external metadata, modernization config, and Operation Log entries. They do not copy external original assets by default.
- Dynamically registered tools must declare capability, permission, input schema, output schema, and audit behavior.

## 7. Prompts

MCP prompt templates should be project-aware but not provider-specific.

```text
prompt.dialogue_polish
prompt.lore_consistency_check
prompt.character_ooc_check
prompt.localization_draft
prompt.qa_route_analysis
```

Prompt inputs should prefer stable IDs:

- scene ID。
- character ID。
- lore ID。
- localization key。
- asset ID。

## 8. Relationship With AI Collaboration

MCP and AI Provider are separate:

- AI Provider generates content.
- MCP exposes project tools and context to an external agent.

When MCP is used by an AI agent:

- Direct writes are allowed only in trusted session.
- Operation Log records tool actions.
- AI Audit Log records AI-assisted content when the tool identifies AI-generated or AI-edited output.
- Review Queue can be used when the workflow wants human approval.

## 9. Testing

Minimum tests:

- Resource path boundary test.
- Mutating tool operation log test.
- Direct write script test.
- Sidecar write and validation test.
- Headless test invocation.
- Cook/package invocation dry test.
- Release gate invocation.
- Secret redaction test.
- Dynamic MCP provider permission and registration test.

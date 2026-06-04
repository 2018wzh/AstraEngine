# ADR 0005: MCP as Agent Capability Protocol

Status: Accepted

## Context

AstraEngine 同时需要开发阶段 Agent 协作和运行时受控生成。MCP 应统一 resources、tools、prompts、sessions 和权限边界，但不能变成 AI Provider 或 Runtime Director。

## Decision

MCP 是 Agent 能力协议层：

- Editor MCP Host 服务项目检查、验证、构建、Review Queue 和 trusted direct write。
- Runtime MCP Host 服务 runtime-safe context、Intent 请求、fallback 和审计。
- Runtime generation 由 Runtime Generation Orchestrator 执行。
- 模型调用由 Provider 模块执行。
- 审计由 Agent Audit 执行。

Runtime MCP 永远没有 project_write 语义。

## Consequences

- Editor trusted write 是显式受信例外，并写 Operation Log。
- Runtime generation 输出 AIIntent，经 Validator、ControlPolicy 和 Director 后才能执行。
- Release Gate 校验 Runtime MCP、Generation、Provider 和 Audit 的 packaged eligibility。

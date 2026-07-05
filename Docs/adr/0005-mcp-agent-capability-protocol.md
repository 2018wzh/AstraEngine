# ADR 0005: MCP 是受控能力协议

## Context

AI 和外部工具需要访问项目、Runtime、Editor 和 Release Gate 能力，但不能绕过权限、审计和回放。

## Decision

MCP tool/resource/prompt 由插件 provider 注册，必须声明 schema、权限、mutating behavior、session requirement、audit 和 rollback policy。

## Consequences

Editor Copilot、Content Generation 和 Runtime AI 可以共享协议，但每条链路有独立权限和 release policy。

# ADR 0006: Text-First Source Data

Status: Accepted

## Context

项目源数据需要适合人类、AI、MCP、Git diff、Review Queue、Cook 和 Release Gate。二进制资源仍需要语义 metadata。

## Decision

Canonical source 使用 YAML + JSON Schema。所有 source object 使用稳定 ID。二进制资源使用同名 `.asset.yaml` sidecar。FilterProfile、Modernization Profile、PluginDescriptor、AI policy、Story Graph、Localization 和 Review Queue 都是文本源数据。

## Consequences

- AssetRegistry、Cooked content、DerivedDataCache 不是人工或 AI 编辑源。
- Release Gate 校验 schema、重复 ID、broken dependency、sidecar、AI-editable 边界和 mount-only policy。
- 外部原游戏资产使用 `foreign-*` AssetId，不伪装为 `native:/`。



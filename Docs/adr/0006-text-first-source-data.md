# ADR 0006: Text-first source, binary runtime data

## Context

创作者和代码审查需要可读、可 diff、可迁移的源数据；发布运行需要快速、完整、可校验的二进制数据。

## Decision

项目源使用 `.astra`、YAML 和少量 JSON authoring metadata。Cook 后生成自描述二进制 package。Save/package section payload 默认使用 postcard/serde。

## Consequences

Editor、CLI、MCP 可以编辑同一源数据；Runtime 不依赖源 YAML 启动。

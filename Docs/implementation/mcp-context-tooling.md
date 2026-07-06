# MCP Context And Tooling

MCP 是外部 AI 工具、Editor 和 Runtime 共享的能力协议。外部工具可以获得 Editor 等价能力，但必须通过 session、permission、audit、undo checkpoint 和 Release Gate。

## Context Pack

```rust
pub struct ContextPack {
    pub id: StableId,
    pub task: ContextTask,
    pub project: ProjectId,
    pub source_refs: Vec<SourceRef>,
    pub diagnostics: Vec<DiagnosticRef>,
    pub memory_cards: Vec<MemoryCardRef>,
    pub asset_refs: Vec<AssetRef>,
    pub redaction: RedactionPolicy,
}
```

Context Pack 是任务入口，不是完整项目 dump。工具可继续调用 read/search capability 拉取 `.astra` source span、schema、diagnostic、release report、save memory 和 asset manifest。返回结果不能包含本地绝对路径、provider secret、未授权商业 payload 或 native handle。

## Tool Capabilities

| Tool | Mutating | Scope | Rule |
| --- | --- | --- | --- |
| `context.read` | false | project/save/package | 只返回授权 source ref 和 section ref |
| `context.search` | false | project/save/package | 返回 ranked refs，不返回无界全文 |
| `memory.read` | false | runtime memory | 遵守 namespace 和 player consent |
| `project.patch` | true | Editor source | 生成 patch、undo checkpoint 和 audit |
| `asset.draft` | true | asset draft sidecar | 生成 draft，不直接进 Cook |
| `command.run` | true | allowlist | 只能运行声明式 check/test/package/report 命令 |
| `release.validate` | true | package/profile | 调用同一 Release Gate validator |

`command.run` 不提供任意 shell。项目可声明 `python Tools/check_docs.py`、`cargo test`、`cargo clippy`、`astra package validate` 这类命令模板；工具只能填入 schema 允许的参数。

## Runtime Endpoint

发布运行时默认只使用内部 MCP session。外部工具连接 runtime endpoint 必须由 release profile、平台权限和用户同意同时开启。外部 runtime tool 不能访问 Editor widget、provider secret 或未提交的创作源。

## Checks

```bash
cargo test -p astra-mcp context_pack
cargo test -p astra-mcp capability_protocol
cargo test -p astra-mcp command_allowlist
cargo test -p astra-release mcp_context_gate
```

Expected report: 未授权 path、未声明命令、外部 runtime endpoint 默认开启、Context Pack 泄露本地绝对路径、mutating tool 缺少 rollback policy 都是 blocking diagnostic。

# AI / MCP Contract

AI 分 Runtime Director、Editor Copilot、Editor Content Generation 和外部工具四条链路。MCP 是能力协议，不是绕过权限、审计、回滚和回放的后门。

## Runtime AI

联网 Runtime AI 可以进入发布包，但 Runtime 不直接持有模型 provider。运行时通过受限 `McpAiSession` 调用模型，发布 profile 必须声明 provider profile、secret handle、network permission、rate policy、IntentValidator、player consent 和 replay policy。AI 输出提交后写入 save/replay：

```rust
pub struct CommittedAiOutput {
    pub intent_id: StableId,
    pub session_id: McpAiSessionId,
    pub provider_profile: ProviderProfileId,
    pub model_fingerprint: String,
    pub prompt_hash: Hash256,
    pub output_hash: Hash256,
    pub payload: BinarySectionRef,
}
```

Replay 使用 committed payload，不重新请求 provider。Live AI provider 在启动时不可用就是 blocking diagnostic，不允许静默切换 provider。

## Runtime Memory

角色和故事记忆由 Engine-owned save/package section 持有。`Canon` 存作者设定和世界观事实，默认只读；`Episodic` 存运行时事件；`Player` 存玩家选择和偏好。创作者通过 policy 限制 AI 可读写的 namespace。短期记忆可以自动压缩归档到长期记忆，写入必须进入 committed memory ledger、audit 和 replay。Embedding/vector index 只是可重建缓存。

## Editor AI

Editor Copilot 和 Content Generation 通过 AI provider profile 集成 OpenAI、Ollama、ComfyUI 等后端。Trusted session 必须显式授权项目、路径范围、操作类型和时长；每次写入生成 patch、audit event、undo checkpoint 和 release gate provenance。

未受信 session 的输出进入 Review Queue。拒绝的 draft 不进入 AssetRegistry、Cook 或 Package。ComfyUI 这类重媒体 workflow 默认只产生 Editor draft sidecar，发布运行时不现场生成重媒体资产。

## MCP Tool Policy

每个 MCP tool 必须声明 mutating behavior、required session、input schema、permission、audit sink、rollback policy 和 packaged eligibility。外部工具可以获得 Editor 等价能力，但所有写入仍走 session、patch、undo、audit 和 Release Gate。Runtime MCP tool 不能访问 Editor widget 或 provider secret。

Context Pack 是外部工具的默认上下文入口。完整项目和 save 通过 `context.read`、`context.search`、`memory.read`、`memory.search` 按需读取；返回内容必须脱敏，不能包含本地绝对路径、provider secret、未授权商业 payload 或 native handle。命令执行只允许声明式 allowlist，不提供任意 shell。

## Trace And Secrets

Editor 凭据保存在用户全局 secret store。Runtime save/package 只保存 `SecretHandle` 或加密引用。AI request、Context Pack、tool result 和输出全文只允许进入本地加密 debug trace；release package/report 只携带 hash、source ref、profile id、consent state 和 provenance。

## Checks

Runtime AI、Editor Copilot、Content Generation 和 MCP tool 的接口与 gate 见 [AI And MCP Runtime Blueprint](../implementation/ai-mcp-runtime.md)。`ai.provider_profile`、`ai.provider_free_replay`、`ai.runtime_memory_policy`、`ai.debug_trace_redaction`、`ai.player_consent`、`mcp.context_permission`、`mcp.command_allowlist` 都是 release check。

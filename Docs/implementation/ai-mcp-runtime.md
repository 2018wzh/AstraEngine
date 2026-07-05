# AI And MCP Runtime Blueprint

AI 能力分为 Runtime AI、Editor Copilot、Content Generation、MCP tool host。共同规则是权限、审计、回滚和 provider-free replay。

## Runtime AI

```rust
pub struct RuntimeAiRequest {
    pub request_id: StableId,
    pub provider: ProviderId,
    pub intent_schema: SchemaId,
    pub prompt_hash: Hash256,
    pub replay_policy: AiReplayPolicy,
}

pub struct CommittedAiOutput {
    pub intent_id: StableId,
    pub provider_id: ProviderId,
    pub model_fingerprint: String,
    pub prompt_hash: Hash256,
    pub output_hash: Hash256,
    pub payload: BinarySectionRef,
}
```

Runtime AI 输出必须通过 IntentValidator，提交后写入 save/replay。Replay 永远读取 committed payload，不请求 provider。

## Editor Copilot

```rust
pub struct TrustedSessionScope {
    pub project: ProjectId,
    pub path_roots: Vec<ProjectPath>,
    pub operations: Vec<EditorOperationKind>,
    pub expires_at_step: u64,
}
```

未授权写入进入 Review Queue。Trusted session 写入也必须生成 patch、audit event、undo checkpoint、release check provenance。

## Content Generation

生成图片、音频、视频、文本或脚本草稿时先写 AI draft sidecar。用户接受后才进入 AssetRegistry、`.astra` 或 Luau policy。被拒绝的 draft 不能进入 Cook 或 Package。

```yaml
schema: astra.ai_draft.v1
id: draft:/image/bg_room_001
provider: astra.ai.provider.example
prompt_hash: sha256:...
output_hash: sha256:...
license_status: review_required
accepted: false
```

## MCP Tool Descriptor

```yaml
schema: astra.mcp_tool.v1
id: astra.tool.release_explain
mutating: false
required_session: project_read
input_schema: astra.release_explain.input.v1
permissions: [release.report.read]
audit_sink: project_audit
rollback: none
packaged: false
```

Mutating MCP tool 必须声明 rollback policy。Runtime MCP tool 不能访问 Editor widget 或 provider secret。

## Checks

```bash
cargo test -p astra-ai runtime_ai_replay
cargo test -p astra-ai editor_copilot
cargo test -p astra-mcp capability_protocol
cargo test -p astra-release ai_mcp_gate
```

Expected report: provider unavailable replay 仍可通过；missing audit、unauthorized path、unaccepted draft 都是 blocking diagnostic。

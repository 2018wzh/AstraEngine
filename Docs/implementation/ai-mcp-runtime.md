# AI And MCP Runtime Blueprint

AI 能力分为 Runtime Director、Editor Copilot、Content Generation、MCP tool host 和 provider profile。共同规则是权限、审计、回滚、加密 trace 和 provider-free replay。

专题页：

- [AI Provider Profiles](ai-provider-profiles.md)：OpenAI、Ollama、ComfyUI、ONNX Runtime 和第一方 provider 插件边界。
- [Runtime AI Director And Memory](runtime-ai-director-memory.md)：运行时 AI 演出、角色记忆、Context Pack 和 Intent。
- [MCP Context And Tooling](mcp-context-tooling.md)：外部工具、上下文读取、命令白名单和 Editor 等价权限。
- [AI Release Gate](ai-release-gate.md)：AI/MCP 发布检查、debug trace 和玩家同意。

## Runtime AI

```rust
pub struct RuntimeAiRequest {
    pub request_id: StableId,
    pub session: McpAiSessionId,
    pub intent_schema: SchemaId,
    pub prompt_hash: Hash256,
    pub replay_policy: AiReplayPolicy,
}

pub struct McpAiSessionScope {
    pub session_id: McpAiSessionId,
    pub project: ProjectId,
    pub allowed_context_roots: Vec<ProjectPath>,
    pub allowed_memory_namespaces: Vec<MemoryNamespace>,
    pub allowed_intents: Vec<SchemaId>,
    pub command_policy: McpCommandPolicy,
}

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

Runtime 不直接持有 OpenAI、Ollama、ComfyUI 或 ONNX Runtime provider。运行时通过受限 `McpAiSession` 请求模型，模型只返回 typed Intent、MCP tool call 或 typed generated artifact chunk。Intent 通过 `IntentValidator` 后写入 committed output、memory ledger、save/replay section，再转成 Event、PresentationCommand 或 memory update。Replay 永远读取 committed payload，不请求 provider。

发布运行时声明 Live AI 时，所需 provider profile 在启动时不可用就是 blocking diagnostic；不静默切换 provider。

`astra-ai-onnx` 的 Runtime 请求还必须绑定 ModelBundle id、VFS mount id、provider profile、platform EP 和 model fingerprint。Provider 只能通过 package reader/VFS/content section ref 打开模型、tokenizer、runtime 和 custom op sidecar，不能读取 loose file 或绝对路径。

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

生成图片、音频、视频、文本或脚本草稿时先写 AI draft sidecar。用户接受后才进入 AssetRegistry、`.astra` 或 Luau policy。被拒绝的 draft 不能进入 Cook 或 Package。ComfyUI 这类重媒体 workflow 默认只属于 Editor content pipeline。

ONNX Runtime local AI 可以在 Shipping Runtime 生成文本、图像或语音，但输出不是 draft sidecar。每个流式 chunk 先通过 IntentValidator 或 artifact validator，再写入 committed output 和 save extra section。正式 replay 使用 save payload；debug/live regeneration 只能作为非权威再生成或差异报告。

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

## Debug Trace

AI request、Context Pack、tool result 和模型输出默认写本地加密 trace，供 Debug profile 复盘。Release package 和 release report 只携带 hash、source ref、provider profile、consent state 和 provenance，不携带明文 prompt、商业文本、玩家自由文本或 provider secret。

Shipping save 不保存完整 prompt 或 Context Pack 明文，只保存生成结果、model fingerprint、validator envelope、content type、chunk hash 和 section ref。大块文本、图像和语音输出使用 save container 的 `Raw` 或 `Zstd` section，可带 `EncryptionDescriptor`。save 体积超过 AI profile 预算只产生 warning；缺少 section hash、migration 或 crypto provider 时按 save/package 规则阻断。

## Checks

```bash
cargo test -p astra-ai runtime_ai_replay
cargo test -p astra-ai editor_copilot
cargo test -p astra-ai runtime_memory
cargo test -p astra-mcp capability_protocol
cargo test -p astra-mcp context_tooling
cargo test -p astra-release ai_mcp_gate
```

Expected report: provider unavailable replay 仍可通过；Live AI provider startup unavailable、missing audit、unauthorized context read、unaccepted draft、missing player consent、ONNX 模型无法通过 package/VFS 解析、CPU fallback 和 release profile 携带明文 trace 都是 blocking diagnostic。

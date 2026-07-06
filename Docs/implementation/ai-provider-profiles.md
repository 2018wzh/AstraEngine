# AI Provider Profiles

AI provider 是 Editor 和 MCP host 的后端适配，不进入 EngineCore，也不被 Runtime 直接持有。Runtime 只看 `McpAiSession`、Context Pack、typed Intent 和 committed output。

## Provider Profile

```rust
pub struct AiProviderProfile {
    pub id: ProviderProfileId,
    pub plugin_id: PluginId,
    pub endpoint_kind: AiEndpointKind,
    pub capabilities: AiCapabilityReport,
    pub secret_policy: AiSecretPolicy,
    pub data_egress: DataEgressPolicy,
    pub runtime_eligible: bool,
    pub debug_trace_policy: DebugTracePolicy,
}

pub enum AiEndpointKind {
    Cloud,
    LocalProcess,
    LocalHttp,
    WorkflowHost,
}
```

`AiCapabilityReport` 声明 language model、embedding、streaming、tool call、asset generation、workflow execution、rate limit、content class、license 和 model fingerprint。项目 manifest 显式绑定 profile；插件加载顺序不能改变选择结果。

Editor 凭据保存在用户全局 secret store。Runtime save/package 只保存 `SecretHandle` 或加密引用，真实 secret 由平台 keystore、部署环境或用户 profile 提供。

## First-party Providers

第一方 provider 插件放在 `Engine/Plugins/Providers` 路线下，默认禁用，项目或 release profile 显式启用。

| Provider | 用途 | Runtime eligibility | Secret policy | CI |
| --- | --- | --- | --- | --- |
| OpenAI | cloud LLM、embedding、tool call、Editor Copilot | 可用于 Runtime MCP session | `SecretHandle` + network egress | fake server contract，真实 smoke opt-in |
| Ollama | local LLM、local embedding、离线创作和运行时 | 可用于 Runtime MCP session | local endpoint，无 cloud secret | local fake endpoint，真实 smoke opt-in |
| ComfyUI | image/video workflow、asset draft sidecar | 默认 Editor-only | local endpoint 或 user secret | workflow fixture，真实 smoke opt-in |

OpenAI 和 Ollama 可以服务 Runtime Director。ComfyUI 默认只生成 Editor draft；作者接受后，资产才进入 AssetRegistry 和 Cook。

## Invocation Boundary

```rust
pub trait AiProvider: StableProvider {
    fn capability(&self) -> ProviderResult<AiCapabilityReport>;
    fn open_session(&self, request: AiSessionRequest) -> ProviderResult<AiSessionId>;
    fn invoke(&self, request: AiInvocationRequest) -> ProviderResult<AiInvocationResult>;
}
```

`AiInvocationRequest` 只包含 ABI-safe value、schema id、context section ref、tool policy ref、secret handle 和 trace policy。Provider 不接收 Actor 指针、Editor widget、RuntimeWorld 指针、native handle 或明文 secret。

## Checks

```bash
cargo test -p astra-ai-provider-openai contract_fake_server
cargo test -p astra-ai-provider-ollama contract_fake_server
cargo test -p astra-ai-provider-comfyui workflow_fixture
cargo test -p astra-release ai_provider_profile_gate
```

Expected report: 未声明 network egress、缺失 secret handle、model fingerprint 缺失、ComfyUI draft 未接受、真实 provider smoke 被放进默认 CI 都是 blocking diagnostic。

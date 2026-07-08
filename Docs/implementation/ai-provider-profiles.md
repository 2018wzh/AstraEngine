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
    PackagedOnnxRuntime,
}
```

`PackagedOnnxRuntime` 表示模型、ONNX Runtime reduced runtime 和可选 custom op sidecar 都来自 package/VFS，而不是任意本地路径。`AiCapabilityReport` 声明 language model、embedding、streaming、tool call、asset generation、workflow execution、rate limit、content class、license、model fingerprint、ModelBundle id、VFS mount id、runtime binary fingerprint 和 execution provider。项目 manifest 显式绑定 profile；插件加载顺序不能改变选择结果。

Editor 凭据保存在用户全局 secret store。Runtime save/package 只保存 `SecretHandle` 或加密引用，真实 secret 由平台 keystore、部署环境或用户 profile 提供。

## First-party Providers

第一方 provider 插件放在 `Engine/Plugins/Providers` 路线下，默认禁用，项目或 release profile 显式启用。

| Provider | 用途 | Runtime eligibility | Secret policy | CI |
| --- | --- | --- | --- | --- |
| OpenAI | cloud LLM、embedding、tool call、Editor Copilot | 可用于 Runtime MCP session | `SecretHandle` + network egress | fake server contract，真实 smoke opt-in |
| Ollama | local LLM、local embedding、离线创作和运行时 | 可用于 Runtime MCP session | local endpoint，无 cloud secret | local fake endpoint，真实 smoke opt-in |
| ComfyUI | image/video workflow、asset draft sidecar | 默认 Editor-only | local endpoint 或 user secret | workflow fixture，真实 smoke opt-in |
| ONNX Runtime | package 内 LLM、embedding、image generation、speech/TTS | 可用于 Runtime MCP session | 无 cloud secret；模型资源走 package/VFS 和可选 section encryption | ModelBundle manifest、VFS/package lookup、真实目标 EP smoke opt-in |

OpenAI、Ollama 和 ONNX Runtime 可以服务 Runtime Director。ComfyUI 默认只生成 Editor draft；作者接受后，资产才进入 Cook，并在 package 阶段写入 VFS manifest/catalog。ONNX Runtime 的 Editor 生成物可以走 draft 流程；Shipping Runtime 生成物必须写入 committed output 和 save section。

## Packaged ONNX Runtime Provider

`astra-ai-onnx` 是一方 provider profile，不进入 EngineCore。原生平台通过 Rust `ort` 绑定 ONNX Runtime；Web 平台通过 ONNX Runtime Web；两者共享同一 `AiProviderProfile`、ModelBundle、MCP session、release gate 和 save/replay 契约。

ModelBundle 是 cook/package 的一等资产，不使用 project-level `package_sections` 携带模型 payload。`ai.model_bundle_manifest` 只记录 id、pipeline、license、fine-tune provenance、redistribution、voice authorization、profile budget、platform targets、model fingerprint、VFS mount id 和 section refs。模型权重、external data、tokenizer、sampler、scheduler、vocoder、pre/post-process config、reduced runtime 和 custom op sidecar 都作为 package/VFS content entry 被 manifest 引用。

Provider 不直接读 loose file 或绝对路径。Bundled、on-demand 和 external 分发都必须落成 `.astrapkg`、patch package、DLC package 或受控 package source，由 Package reader 和 VFS 提供稳定 section ref。模型、runtime、custom op sidecar 和生成输出的加密复用 `EncryptionDescriptor`；没有匹配 crypto provider 时，按 package diagnostic 阻断，不设计模型专用 DRM。

Shipping local AI 固定主 execution provider：Windows `DirectML`、Linux `OpenVINO`、macOS/iOS `CoreML`、Android `QNN`、Web `WebNN`。目标平台缺主 EP、operator coverage 不足、触发 CPU fallback 或缺真实目标运行报告时，该平台 local AI Shipping profile blocking。开发期可以下载 runtime；release 只消费 Engine recipe 锁定的 vendor cache。

项目可以自管 ORT custom op sidecar。sidecar 必须作为 package/VFS entry 声明平台二进制、hash、license、加载策略和目标运行证据；它不能暴露或接收 Engine object、Actor 指针、Editor widget、GPU/audio native handle、provider trait object 或本地路径。

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
ONNX Runtime 还要阻断缺失 ModelBundle manifest、模型 payload 走 `package_sections` 旁路、VFS section ref 无法解析、缺主 EP、CPU fallback、缺 vendor cache lock、custom op sidecar 未声明 hash/license 或 release report 泄露 payload。

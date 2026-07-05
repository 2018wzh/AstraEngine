# AI / MCP Contract

AI 分 Runtime AI、Editor Copilot、Editor Content Generation 三条链路。MCP 是能力协议，不是绕过权限和审计的后门。

## Runtime AI

联网 Runtime AI 可以进入发布包，但必须声明 provider、secret、network permission、rate policy、fallback、IntentValidator 和 replay policy。AI 输出提交后写入 save/replay：

```rust
pub struct CommittedAiOutput {
    pub intent_id: StableId,
    pub provider_id: ProviderId,
    pub model_fingerprint: String,
    pub prompt_hash: Hash256,
    pub output_hash: Hash256,
    pub payload: BinarySectionRef,
}
```

Replay 使用 committed payload，不重新请求 provider。

## Editor AI

Editor Copilot 和 Content Generation 可以在 Trusted session 中直写 canonical source。Trusted session 必须显式授权项目、路径范围、操作类型和时长；每次写入生成 patch、audit event、undo checkpoint 和 release gate provenance。

未受信 session 的输出进入 Review Queue。拒绝的 draft 不进入 AssetRegistry、Cook 或 Package。

## MCP Tool Policy

每个 MCP tool 必须声明 mutating behavior、required session、input schema、permission、audit sink、rollback policy 和 packaged eligibility。Runtime MCP tool 不能访问 Editor widget 或 provider secret。

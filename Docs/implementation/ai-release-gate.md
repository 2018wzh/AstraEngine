# AI Release Gate

AI/MCP gate 检查 provider profile、Runtime Director、memory、MCP tool、debug trace、玩家 consent 和 provider-free replay。Editor、CLI、CI 和 MCP 调用同一 validator。

## Required Checks

| Check ID | Input | Blocking Condition | Evidence |
| --- | --- | --- | --- |
| `ai.provider_profile` | provider descriptor、project binding | fingerprint、secret、network egress、runtime eligibility 或 model fingerprint 缺失 | provider id、profile id、model fingerprint |
| `ai.model_bundle` | ModelBundle manifest、package section table | manifest 缺失、模型 payload 走 `package_sections` 旁路、section ref/hash/codec/migration 缺失、license/provenance/redistribution 缺失 | bundle id、section id、hash、license status |
| `ai.onnx_runtime_pack` | runtime vendor cache、package/VFS mount | reduced runtime 未锁定、release 阶段联网拉取 runtime、VFS mount 无法解析、custom op sidecar 缺 hash/license/平台声明 | runtime fingerprint、VFS mount id、sidecar id |
| `ai.onnx_execution_provider` | platform capability、真实目标运行报告 | 主 EP 缺失、operator coverage 不足、发生 CPU fallback、缺真实目标运行证据 | platform id、EP、model fingerprint、operator coverage |
| `ai.runtime_provider_startup` | release profile、platform capability | Live AI 所需 provider 启动不可用 | provider profile、platform id、diagnostic |
| `ai.provider_free_replay` | save/replay | replay 需要请求 provider | committed output hash |
| `ai.generated_artifact_save` | save section、committed output | 生成 chunk 未写 save extra section、artifact manifest 缺映射、hash/migration/encryption 不完整 | artifact section id、chunk hash、validator status |
| `ai.runtime_memory_policy` | memory ledger、policy | 越权写 canon、缺失 ledger、index 被当成权威来源 | namespace、entry hash、policy id |
| `ai.debug_trace_redaction` | package/report/debug profile | release artifact 携带明文 prompt、玩家文本、商业 payload 或 secret | trace id、redaction status |
| `ai.player_consent` | runtime profile、save memory | 云端 provider 读取玩家 memory 但缺少首启同意记录 | consent id、provider profile |
| `mcp.context_permission` | MCP audit | read/search/tool call 越权或 Context Pack 未脱敏 | session id、tool id、source ref |
| `mcp.command_allowlist` | MCP command report | 执行未声明命令或任意 shell | command id、template id |

## Report Shape

```yaml
schema: astra.release_report.v1
checks:
  - id: ai.provider_profile
    domain: ai_mcp
    status: pass
    evidence:
      provider_profile: astra.provider.openai
      model_fingerprint: hash256:...
  - id: ai.player_consent
    domain: ai_mcp
    status: blocked
    diagnostic: ASTRA_AI_PLAYER_CONSENT_MISSING
    evidence:
      memory_namespace: player.default
  - id: ai.onnx_execution_provider
    domain: ai_mcp
    status: blocked
    diagnostic: ASTRA_AI_ONNX_CPU_FALLBACK
    evidence:
      provider_profile: astra.provider.onnx
      model_bundle: com.example.model.local_director
      platform: windows
      required_ep: DirectML
      observed_ep: CPU
```

Release report 只写 hash、source ref、profile id、consent state、VFS mount id、section id、model fingerprint、EP、budget 和 diagnostic。全文 prompt、Context Pack 明文、模型 payload、custom op payload、玩家自由文本、商业内容、本地路径和 provider secret 只允许进入本地加密 debug trace，不能进入 release artifact。

## Checks

```bash
cargo test -p astra-release ai_provider_profile_gate
cargo test -p astra-release ai_runtime_memory_gate
cargo test -p astra-release ai_mcp_gate
```

Expected report: provider 失配、Live AI provider 不可用、ONNX ModelBundle 缺 manifest、VFS/package section 无法解析、release 联网拉取 runtime、CPU fallback、custom op sidecar 缺声明、generated artifact 未进入 save、缺失玩家同意、明文 trace 进入 release artifact、MCP command 越权都是 blocking diagnostic。AI save 体积超过 profile 预算只输出 warning，不阻断发布。

# AI Release Gate

AI/MCP gate 检查 provider profile、Runtime Director、memory、MCP tool、debug trace、玩家 consent 和 provider-free replay。Editor、CLI、CI 和 MCP 调用同一 validator。

## Required Checks

| Check ID | Input | Blocking Condition | Evidence |
| --- | --- | --- | --- |
| `ai.provider_profile` | provider descriptor、project binding | fingerprint、secret、network egress、runtime eligibility 或 model fingerprint 缺失 | provider id、profile id、model fingerprint |
| `ai.runtime_provider_startup` | release profile、platform capability | Live AI 所需 provider 启动不可用 | provider profile、platform id、diagnostic |
| `ai.provider_free_replay` | save/replay | replay 需要请求 provider | committed output hash |
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
```

Release report 只写 hash、source ref、profile id、consent state 和 diagnostic。全文 prompt、Context Pack 明文、玩家自由文本和 provider secret 只允许进入本地加密 debug trace。

## Checks

```bash
cargo test -p astra-release ai_provider_profile_gate
cargo test -p astra-release ai_runtime_memory_gate
cargo test -p astra-release ai_mcp_gate
```

Expected report: provider 失配、Live AI provider 不可用、缺失玩家同意、明文 trace 进入 release artifact、MCP command 越权都是 blocking diagnostic。

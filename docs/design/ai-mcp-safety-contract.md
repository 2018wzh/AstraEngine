# AI / MCP Safety Contract

状态：Production contract draft / AI and MCP runtime safety not implemented  
定位：定义 Runtime AI intent、Editor Copilot、Content Generation、review/audit/save replay 的安全契约。AI provider 不进入 Core；Runtime replay 不重新请求模型。

## 1. 目标

- Runtime AI 只能提交结构化 `AIIntent`，经 IntentValidator、Director 和 ControlPolicy 仲裁后转为 RuntimeEvent。
- Editor Copilot 输出 patch proposal 或 review item，不默认直接写 canonical source。
- Content Generation draft 未 accepted 前不能进入 AssetRegistry、Cook 或 Package。
- Committed AI output 进入 Save/Replay 和 audit log，保证 provider-free replay。

非目标：

- AI 不直接跳剧情、不直接改核心变量、不直接调用底层 renderer/audio/filesystem API。
- Runtime MCP 不能 project write。

## 2. Runtime AI Intent

Intent:

```yaml
schema: astra.ai.intent.v1
intent_id: intent:/runtime/feedback/42
source: runtime_feedback
actor: actor:/systems/ai_director
requested_events:
  - type: event:/vn.dialogue.suggest
payload_schema: astra.vn.ai.dialogue_suggestion.v1
payload: {}
audit_ref: audit:/ai/42
```

Validation result:

```yaml
schema: astra.ai.intent_validation_result.v1
intent_id: intent:/runtime/feedback/42
decision: commit
director_result: allow
control_policy_result: allow
committed_output_id: committed-ai:/42
diagnostics: []
```

Committed output:

```yaml
schema: astra.ai.committed_output.v1
id: committed-ai:/42
provider_id: astra.ai.provider.local
input_hash: "..."
output_hash: "..."
accepted_frame: 120
replay_payload: {}
```

Rules:

- Replay uses `replay_payload`; it must not invoke provider.
- Deterministic release profile blocks runtime AI provider unless explicitly allowed.
- AIIntent cannot bypass Director phase or channel lock.

## 3. Editor Copilot And Content Generation

Patch proposal:

```yaml
schema: astra.ai.patch_proposal.v1
proposal_id: patch:/copilot/opening-dialogue
source_files: []
changes: []
review_required: true
trusted_session_required: false
```

Generation audit:

```yaml
schema: astra.ai.generation_audit.v1
audit_id: audit:/draft/alice-sprite
provider_id: astra.ai.image.local
prompt_hash: "..."
context_hash: "..."
output_hash: "..."
license_policy: project_review_required
review_target: native:/Characters/Alice/Draft
```

Review item:

```yaml
schema: astra.review.item.v1
review_id: review:/draft/alice-sprite
kind: asset_draft
status: pending
required_approvers: []
diagnostics: []
```

Rules:

- Drafts live under Saved/Agent or equivalent draft workspace.
- Accepted draft import must produce sidecar and audit link.
- Rejected draft is not visible to Cook.

## 4. MCP Host Boundaries

Runtime MCP tools:

- inspect runtime snapshot.
- submit player feedback.
- request/validate/commit AI intent.
- select fallback.

Editor MCP tools:

- explain diagnostics.
- propose patches.
- run validation.
- create review item.
- generate/import draft through review.

Rules:

- Runtime MCP cannot write project source.
- Editor mutating tools require trusted session or Review Queue.
- All MCP operations emit operation log records.

## 5. Release Gate And Acceptance

Release Gate blocks:

- unreviewed AI draft in Content.
- deterministic profile with unauthorized runtime AI provider.
- committed output without audit record.
- provider requiring network/secret in disallowed profile.

`AIIntentSafety` acceptance:

- AIIntent validate/commit/save/replay path.
- provider-free replay.
- rejected draft excluded from Cook.
- trusted vs review-required write policy covered.




# AI 协作与运行时受控内容设计

## 1. 目标

AI 分为三套独立工作流，均通过 MCP 暴露能力，但 session、权限、审计和发布策略不同：

- `Runtime AI MCP`：根据玩家反馈和运行时上下文实时生成受控内容。
- `Editor Copilot MCP`：类似 Copilot 的创作全流程辅助，生成建议、解释和 patch proposal。
- `Editor Content Generation MCP`：创作期内容生成、修改和增强，产出 draft，经 Review Queue 后进入 canonical source。

AI 不直接调用底层 Renderer、Audio、Asset、Script native API；不直接跳剧情；不直接修改核心变量；
不直接创建未授权正式资产。Runtime AI 的提交结果必须可保存和回放；Editor AI 的正式写入必须经过
Review Queue 或显式 trusted session。

## 2. Runtime AI MCP

Runtime AI MCP 只存在于允许 runtime generation 的发布模式。它根据玩家反馈、Actor 状态、
剧情阶段、Blackboard、ControlPolicy、Director、Canon 和 fallback policy 生成结构化内容。

```text
Player Feedback / Runtime Event
  -> Runtime MCP Host
  -> Runtime Context Builder
  -> RuntimeGenerationOrchestrator
  -> AI Provider
  -> AIIntent / RuntimeContentDraft
  -> IntentValidator
  -> Director + ControlPolicy
  -> Commit
  -> RuntimeEvent / VNEvent / PresentationCommand
  -> Save / Replay committed output
```

### Runtime Resources

```text
astra://runtime/session
astra://runtime/world
astra://runtime/scene
astra://runtime/actors/{id}
astra://runtime/blackboards/{scope}
astra://runtime/director
astra://runtime/control-policy/{actorId}
astra://runtime/constraints
astra://runtime/feedback
astra://runtime/fallbacks
astra://runtime/committed-ai-output
astra://runtime/save-preview
```

### Runtime Tools

- `runtime.context.inspect`：读取 runtime-safe context。
- `runtime.feedback.submit`：提交玩家反馈、偏好、自由输入或互动结果。
- `runtime.intent.request`：请求生成结构化 `AIIntent`。
- `runtime.intent.validate`：调用 IntentValidator，返回 allow/deny/requires_fallback。
- `runtime.intent.commit`：提交已验证 intent，写入 committed output。
- `runtime.fallback.select`：当 provider 不可用或 intent 被拒绝时选择确定性 fallback。
- `runtime.audit.annotate`：写 runtime generation audit annotation。

### Runtime Permissions

- 允许：读取 runtime-safe context、请求 provider、提交已验证 committed output。
- 禁止：project write、canonical source mutation、读取未授权外部路径、直接写 AssetRegistry、直接执行 provider raw output。
- 必须：经 IntentValidator、Director、ControlPolicy、Save/Replay。

### Runtime Data Contracts

```cpp
struct RuntimeFeedback {
    RuntimeFeedbackId id;
    ActorId source_actor;
    StringView channel;
    StringView text;
    Json payload;
};

struct AIIntent {
    AIIntentId id;
    StringView type;
    ActorId target_actor;
    RuntimeEventCategory output_category;
    Json payload;
    AuditId generation_audit;
};

struct CommittedAIOutput {
    AIIntentId intent_id;
    Json committed_payload;
    AuditId generation_audit;
    uint64_t replay_sequence;
};
```

### Runtime Save / Replay

- Save 存储 `CommittedAIOutput`、generation audit reference、fallback choice 和 replay sequence。
- Replay 使用 committed output，不重新请求 provider。
- Runtime AI mismatch 必须定位到 feedback id、intent id、validator result、commit sequence。

## 3. Editor Copilot MCP

Editor Copilot MCP 面向创作全流程辅助，类似 Copilot。它帮助作者理解、修改和验证项目，
默认只产出 suggestion 或 patch proposal。

```text
Author Request
  -> Editor MCP Host
  -> Project / Asset / Script / Graph / Timeline Context
  -> Provider
  -> Suggestion / Patch Proposal / Diagnostics Explanation
  -> Review Queue
  -> Apply only by review or trusted session
```

### Copilot Resources

```text
astra://project/manifest
astra://project/config
astra://scripts/{path}
astra://graphs/{id}
astra://timelines/{id}
astra://assets/registry
astra://lore/{id}
astra://diagnostics
astra://release-gate/report
astra://review-queue
astra://audit
```

### Copilot Tools

- `project.inspect`
- `project.plan_patch`
- `script.suggest`
- `graph.suggest`
- `timeline.suggest`
- `filtergraph.suggest`
- `diagnostics.explain`
- `schema.fix_proposal`
- `test.run_headless`
- `build.cook`
- `release.run_gate`
- `review.enqueue`
- `review.apply`

Mutating tools such as `project.apply_patch`, `project.write_file` and `review.apply`
require trusted session and Operation Log.

### Copilot Data Contracts

```cpp
struct AIEditRequest {
    StringView user_intent;
    Span<TargetRef> targets;
    Span<CapabilityId> allowed_operations;
    ReviewPolicy review_policy;
};

struct AIPatchProposal {
    StringView summary;
    StructuredDiff diff;
    Span<Diagnostic> diagnostics;
    Span<RiskNote> risks;
    AuditId generation_audit;
};
```

## 4. Editor Content Generation MCP

Editor Content Generation MCP 面向创作期内容生成、修改和增强。它产出 draft，不直接写正式 Content。

支持内容：

- 文本、对白、设定、localization。
- 图像、角色、背景、UI overlay。
- 音频、语音、音乐、音效。
- 视频、动画、timeline suggestion。
- FilterProfile、现代化 profile、sidecar metadata。

工作流：

```text
Generation / Modification / Enhancement Request
  -> Editor MCP Host
  -> Boundary Manager
  -> Context Builder
  -> Provider
  -> Draft Binary/Text + Sidecar Draft
  -> Preview / Variants
  -> Review Queue
  -> Accept / Edit / Reject / Regenerate
  -> Canonical Source + Sidecar
  -> AssetRegistry / Cook
```

### Content Generation Tools

- `asset.generate_draft`
- `asset.modify_draft`
- `asset.enhance_draft`
- `asset.preview_draft`
- `asset.compare_variants`
- `asset.import_draft`
- `asset.validate`
- `review.enqueue`
- `review.apply`

### Draft Data Contracts

```cpp
struct AIAssetGenerationRequest {
    AssetType asset_type;
    StringView prompt;
    Span<AssetId> references;
    ProjectPath target_folder;
    LicenseProfile license_profile;
    OutputConstraints output_constraints;
};

struct AIAssetDraft {
    DraftId draft_id;
    AssetType asset_type;
    ProjectPath temporary_output;
    SidecarDocument sidecar_draft;
    PreviewMetadata preview;
    AuditId generation_audit;
};

struct AIReviewItem {
    ReviewItemKind kind;
    StringView summary;
    AuditId generation_audit;
    Optional<OperationId> applied_operation;
};
```

Draft import 必须生成稳定 `native:/` AssetId、sidecar、license、review 状态和 audit 链接。
被取消或拒绝的 draft 不写入正式 Content，不进入 AssetRegistry，不参与 Cook。

## 5. Boundary Manager

Boundary Manager 合并 project policy、stage policy、release profile、session permission、provider capability 和 audit requirement。

```yaml
default_mode: assistant
editor_copilot:
  allow_inline_suggestions: true
  allow_patch_proposals: true
  allow_trusted_write: false
editor_content_generation:
  allow_text: true
  allow_image: true
  allow_audio: true
  allow_voice: true
  allow_video: true
  allow_enhancement: true
  require_review: true
runtime_ai_mcp:
  allow_runtime_generation: false
  allow_dialogue: true
  allow_state_change: limited
  allow_story_jump: false
  require_committed_output: true
```

许可结果必须包含 allowed、requires_review、allowed_targets、blocked_targets、release_mode 和 reason。

## 6. Provider 模块

Provider 是独立动态模块，不等同于 MCP Host、RuntimeGenerationOrchestrator 或 Agent Audit。

```cpp
class IAIProvider {
public:
    virtual AIProviderInfo info() const = 0;
    virtual Expected<AIResponse, AIError> complete(const AIRequest& request) = 0;
    virtual Expected<StreamHandle, AIError> stream_complete(const AIRequest& request) = 0;
};
```

Provider 必须声明：

- modality：text、image、audio、voice、video、animation、multimodal。
- network/offline。
- editor eligibility。
- runtime eligibility。
- packaged eligibility。
- secret requirements。
- streaming support。
- audit fields。

## 7. Agent Audit

- Operation Log：工具副作用，如 trusted write、review apply、asset import、build/cook/package。
- Generation Audit Log：prompt/context/output hash、provider、fallback、session、最终 patch、asset draft 或 committed intent。

Runtime AI 输出一旦 commit，必须保存为确定性数据。Editor draft 一旦 import，必须保留 provenance。

## 8. Release Modes

```text
Deterministic Build
  固定内容，无 Runtime AI Provider；允许已审核并进入 Content 的 AI 资产。

Hybrid Build
  固定主线 + 受控 runtime feedback generation；包含 Runtime MCP、Provider、Audit、Fallback。

Experimental Build
  更高自由度，仅用于研究或测试；必须显式标记。
```

Release Gate：

- 阻止未审核 AI draft。
- 阻止 deterministic build 包含 runtime AI provider。
- 校验 Runtime MCP tool 权限、provider packaged eligibility 和 committed output policy。
- 校验 Editor MCP trusted write 不进入 packaged runtime。

## 9. 验收

- Runtime AI 能根据玩家反馈生成 intent、验证、commit、保存、回放，回放不访问 provider。
- Editor Copilot 能提出脚本或 Graph 修复，进入 Review Queue，并只在 trusted session 中应用。
- Editor Content Generation 能生成/修改/增强资产 draft，review 接受后导入 Content 并通过 Release Gate。
- Deterministic Build 阻止未审核 draft 和 runtime AI provider。

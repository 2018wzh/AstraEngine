# AI 协作与运行时受控内容设计

## 1. 目标

AI 是创作协作层和受控运行时内容来源，不是作品接管者。AstraEngine 支持两条 AI 链路：

- Editor AI Collaboration：开发阶段建议、生成草稿、校验和 Review Queue。
- Runtime AI Intent：运行时根据上下文生成结构化 Intent，经 Validator、ControlPolicy 和 Director 审核后转成事件。

AI 不直接调用底层 API，不直接跳转剧情，不直接修改核心变量，不直接创建未授权资产。

## 2. Editor AI 工作流

Editor AI Collaboration 默认是 assistant mode，类似 Copilot：

- Inline suggestion：在脚本、Graph、Timeline、Inspector 字段旁给出补全、解释和替换建议。
- Chat-driven patch：根据作者请求生成结构化 patch proposal，不立即写入正式源数据。
- Batch refactor：对一组脚本、资产 metadata 或本地化条目提出批量变更。
- Diagnostics explanation：解释 schema、Release Gate、Cook、测试或 runtime diagnostic，并给出修复建议。
- Asset generation request：从 Asset Editor 或 AI Workbench 发起多模态资产草稿生成。

```text
Author Request
  -> Boundary Manager
  -> Context Builder
  -> Provider Module
  -> Schema Validation
  -> Suggestion / Patch / Draft
  -> Review Queue
  -> Accept / Edit / Reject
  -> Canonical Text Source
  -> Agent Audit
```

AI 输出默认是建议。正式内容必须有人类接受或编辑后进入项目源数据。Editor trusted direct write 是显式受信例外，仍必须记录 Operation Log。

### 2.1 Patch / Review

AI patch 必须表达为可审查 proposal，而不是直接覆盖文件：

```text
AIEditRequest
  -> AIPatchProposal
  -> AIReviewItem
  -> Preview Diff / Validate
  -> Accept / Edit / Reject
  -> Apply To Canonical Source
```

目标态接口草案：

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

struct AIReviewItem {
    ReviewItemKind kind;
    StringView summary;
    AuditId generation_audit;
    Optional<OperationId> applied_operation;
};
```

`AIReviewItem` 统一承载 patch、asset draft、localization draft 和 runtime intent preview。Review Queue 接受后才写入 canonical source；被拒绝的 draft 只能保留审计和临时预览，不进入 AssetRegistry 或 Cook。

### 2.2 Trusted Direct Write

Trusted direct write 用于作者明确授权 AI 代劳完成项目修改。它不是默认模式：

- Session 必须显式开启 `trusted_write`，并绑定 caller、project、policy snapshot、capability set 和 audit sink。
- 写入范围限制在 workspace/project 内的 canonical source，不允许写 Cooked、DerivedDataCache、package manifest 或外部 mount-only 原始资产。
- 每次写入必须先产出 patch proposal，再由 tool apply；生成写 Generation Audit，实际文件副作用写 Operation Log。
- Operation Log 必须包含原始内容 hash、结果 hash、目标路径、session、provider 和可回滚记录。
- Trusted write 仍受 Boundary Manager、schema validation、Review Queue policy 和 Release Gate 检查。

### 2.3 Asset Generation

Asset Editor 和 AI Workbench 可发起多模态生成：文本、图像、音频、语音、视频或动画草稿。生成结果默认是 draft，不是正式资产。

```text
Asset Generation Request
  -> Boundary Manager
  -> Context Builder
  -> Provider Module
  -> Draft Binary / Text Output
  -> Sidecar Draft
  -> Preview / Variants
  -> Review Queue
  -> Import / Edit / Reject
  -> Canonical Source + Asset Sidecar
  -> Agent Audit
```

目标态接口草案：

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
```

Draft import 必须生成稳定 `native:/` AssetId、sidecar、license、review 状态和审计链接。被取消或拒绝的 draft 不写入正式 Content，不进入 AssetRegistry，不参与 Cook。

## 3. Runtime AI Intent 工作流

```text
Player Input
  -> Interaction StateMachine
  -> Runtime Context Builder
  -> RuntimeGenerationOrchestrator
  -> AI Planner / Provider
  -> Structured AIIntent
  -> IntentValidator / Guard
  -> Director + ControlPolicy
  -> RuntimeEvent / VNEvent
  -> Actor StateMachine
  -> PresentationCommand
```

示例 Intent：

```json
{
  "type": "DialogueIntent",
  "speaker": "alice",
  "emotion": "nervous",
  "text": "我不确定你该不该知道这件事。"
}
```

Validator 检查：

- Actor 是否存在且在场。
- 当前剧情阶段是否允许自由对话。
- Timeline 或 Story Script 是否锁定该 Actor。
- AI 是否允许改变情绪、变量或关系值。
- 是否违反 Canon Lore、年龄分级、角色设定或发布策略。

## 4. Boundary Manager

Boundary Manager 合并项目策略、阶段策略、内容权限、运行时策略和审计要求：

```yaml
default_mode: assistant
allow_ai_modify_canon: false
require_human_approval: true
allow_runtime_generation: false
editor:
  allow_inline_suggestions: true
  allow_patch_proposals: true
  allow_trusted_write: false
asset_generation:
  allow_text: true
  allow_image: true
  allow_audio: true
  allow_voice: true
  allow_video: true
  require_review: true
runtime_intent:
  allow_dialogue: true
  allow_state_change: limited
  allow_story_jump: false
```

许可结果必须包含 allowed、requires_review、allowed_targets、blocked_targets 和 reason。

## 5. Provider 模块

Provider 是独立动态模块，不与 MCP、Runtime Generation 或 Audit 混合：

```cpp
class IAIProvider {
public:
    virtual AIProviderInfo info() const = 0;
    virtual Expected<AIResponse, AIError> complete(const AIRequest& request) = 0;
    virtual Expected<StreamHandle, AIError> stream_complete(const AIRequest& request) = 0;
};
```

Provider 必须声明网络、离线、文本/图像/音频、多模态、运行时使用和 packaged eligibility。

## 6. Agent Audit

审计分两类：

- Operation Log：工具副作用，如 trusted write、build、validation、runtime tool call。
- Generation Audit Log：prompt/context/output hash、provider、fallback、session、最终 patch、asset draft 或 committed intent。

运行时 AI 输出一旦提交，必须保存为确定性数据。回放优先使用 committed output，不重新请求模型。

## 7. 发布模式

```text
Deterministic Build
  固定内容，无运行时 AI，默认商业发布模式。

Hybrid Build
  固定主线 + 受控闲聊/反应，包含 Runtime MCP、Generation、Provider、Audit。

Experimental Build
  更高自由度，仅用于研究或测试。
```

Release Gate 必须阻止未审核 AI 内容进入 Deterministic Build，并校验 Runtime AI 模块权限。

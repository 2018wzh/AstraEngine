# AI 协作与运行时受控内容设计

## 1. 目标

AI 是创作协作层和受控运行时内容来源，不是作品接管者。AstraEngine 支持两条 AI 链路：

- Editor AI Collaboration：开发阶段建议、生成草稿、校验和 Review Queue。
- Runtime AI Intent：运行时根据上下文生成结构化 Intent，经 Validator、ControlPolicy 和 Director 审核后转成事件。

AI 不直接调用底层 API，不直接跳转剧情，不直接修改核心变量，不直接创建未授权资产。

## 2. Editor AI 工作流

```text
Author Request
  -> Boundary Manager
  -> Context Builder
  -> Provider Module
  -> Schema Validation
  -> Diff / Patch
  -> Review Queue
  -> Accept / Edit / Reject
  -> Canonical Text Source
  -> Agent Audit
```

AI 输出默认是建议。正式内容必须有人类接受或编辑后进入项目源数据。Editor trusted MCP direct write 是显式受信例外，仍必须记录 Operation Log。

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
- Generation Audit Log：prompt/context/output hash、provider、fallback、session、最终 patch 或 committed intent。

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

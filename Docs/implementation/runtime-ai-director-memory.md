# Runtime AI Director And Memory

Runtime AI v1 覆盖 Director + 角色记忆。它可以生成对话、选择、演出节拍和 episodic memory update，但不能执行自由脚本，也不能绕过 Runtime tick、MutationLog、Save container 或 Release Gate。

## Director Flow

```text
RuntimeWorld tick
  -> build RuntimeAiIntentRequest
  -> McpAiSession context read/search
  -> provider invocation
  -> typed RuntimeAiIntent
  -> IntentValidator
  -> committed output + memory ledger
  -> EventQueue / PresentationCommand / save section
```

```rust
pub struct RuntimeAiIntent {
    pub id: StableId,
    pub schema: SchemaId,
    pub actor_scope: Vec<ActorId>,
    pub memory_updates: Vec<MemoryUpdate>,
    pub presentation_beats: Vec<PresentationBeat>,
    pub dialogue: Vec<DialogueIntent>,
    pub choices: Vec<ChoiceIntent>,
}
```

`IntentValidator` 校验 schema、角色权限、剧情阶段、memory namespace、presentation capability 和 content policy。失败结果写 diagnostic，不进入 Runtime state。

## Memory Model

```rust
pub struct MemoryEntry {
    pub id: StableId,
    pub namespace: MemoryNamespace,
    pub authority: MemoryAuthority,
    pub layer: MemoryLayer,
    pub source_ref: SourceRef,
    pub content_ref: BinarySectionRef,
    pub summary_hash: Hash256,
}

pub enum MemoryAuthority {
    Canon,
    Episodic,
    Player,
}

pub enum MemoryLayer {
    Working,
    ShortTerm,
    LongTerm,
    Archive,
}
```

`Canon` 存作者设定、世界观事实和故事规则，默认只读。`Episodic` 存运行时事件和角色经验。`Player` 存玩家选择、偏好和个性化信息，进入 save memory。

创作者通过 policy 限制 AI 可读写的 namespace。短期记忆可以按策略自动压缩归档到长期记忆；归档写入 committed memory ledger、audit 和 replay。Embedding/vector index 只是可重建缓存，不拥有权威数据。

## Context Access

模型初始 prompt 只拿最小 Context Pack。完整项目和 save 通过 MCP `context.read`、`context.search`、`memory.read` 和 `memory.search` 按需读取。每次读取都记录 source ref、namespace、token budget、脱敏级别和 session id。

玩家 memory 可以发给云端 provider，但 release profile 必须声明该能力，平台壳首次运行必须展示可见同意并记录 consent state。

## Checks

```bash
cargo test -p astra-ai runtime_director_intent
cargo test -p astra-ai runtime_memory
cargo test -p astra-ai memory_compaction_replay
cargo test -p astra-release ai_memory_policy_gate
```

Expected report: AI 修改 canon 超出授权、memory index 变成权威来源、玩家 consent 缺失、压缩归档不进 ledger、replay 重新请求 provider 都是 blocking diagnostic。

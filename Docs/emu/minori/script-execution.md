# Minori Script Execution

本阶段没有 VM。现有 CFG 只表达 parser 已确认的 `label`、`goto`、`if` 和跨脚本 `chain`；`end` 暂记 terminate，样本未观察到可独立确认的 return command。`select` 的 operand/target 关系仍为 unknown。parser 不能执行变量、等待、消息、音频或演出命令。

全包 census 已确认 29 个 command token 及其数量。下一阶段仍需逐项拆分入口、消息、姓名、语音、选项、条件表达式、变量、等待和主要演出 operand；没有样本证据的字段保持 unknown，不用 command 名称代替语义证明。执行核心仍须经 `LegacyRuntimeProvider` session 和 RuntimeWorld StateMachine 提交 effect/await/trace。

## VM State

Minori core 持有 family 私有状态：

```text
pc
call_stack
variables
flags
message_state
choice_state
presentation_layers
audio_state
resource_cache_refs
```

Manager 只能接收 trace 和 presentation/audio command，不读取私有 VM 内存。

## Tick

每个 tick 执行到以下暂停点之一：

- `Wait(duration)` 未结束。
- `WaitInput` 等待用户推进。
- `ChoiceGroup` 等待选择。
- movie/audio 同步点。
- save/load snapshot 边界。
- fatal diagnostic。

可挂起动作保存为 `AwaitToken`，恢复时在固定 tick 边界进入事件队列。

## Save/Load

Snapshot 包含 VM state、当前脚本文件、pc、message/backlog、已提交 presentation layer、audio loop 状态和 patch mount manifest。Snapshot 不包含解密 payload。

## Determinism

随机数、auto/skip、voice replay 和 movie end event 都必须进入 trace。联网或系统时间不参与脚本决定。

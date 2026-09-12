# Runtime Execution

Runtime 用固定 tick 接收有序输入和异步完成结果。Actor/Component 可由持有 World 的宿主直接更新；flat StateMachine 是可选执行机制。完整接口与失败边界见 [Runtime Contract](../contracts/runtime.md)。

## Tick 顺序

```text
TickRequest
  -> 校验连续 step、seed、delta、mode、required slots 与 ingress 顺序
  -> 提交 typed PlayerInput / AwaitCompletion
  -> 处理到期 AwaitToken 和 delayed event
  -> 执行 StateMachine 候选 action，验证后提交
  -> 返回 step、integrity mode 与 diagnostics
```

实时 tick 不生成 aggregate state/event/presentation 摘要，也不为整帧建立 snapshot 或逆向日志。输入预检失败不修改 World；执行失败或 panic 终止 World，已提交状态保留供诊断，后续 tick、写入和保存拒绝，直到销毁或成功读档。

## Event 与 Await

EventQueue 按 `(step, sequence, id)` 消费事件；DelayedEventQueue 按 `(due_tick, sequence, id)` 把到期事件加入同一队列。队列和 StableId generator 进入存档，读档后继续使用保存的 sequence。

现有 AwaitToken 使用显式 token id、请求 step、可选 timeout step 和完成策略。`AwaitCompletionPolicy::HostResult` 表示接收持有当前完成句柄的 host 结果；`TickTimeout` 只在指定 tick 超时，拒绝外部 completion。任务作用域和完成句柄的取消/读档边界见 Runtime Contract。不能把 Future 或 native handle 放入存档。

## 候选状态与诊断

StateMachine action 使用 `DeterministicActionContext`，必须声明实际 read/write set。单个 machine 的候选改动通过验证后提交；这不提供整个 World 的失败回滚。跨 machine 执行错误会终止本次会话，不能以历史 hash 或撤销日志伪装原子成功。

World 目前仍保留 event、presentation、machine 和 mutation 诊断记录，以服务现有 DebugSession。通用 replay recorder/transcript、Replay tick mode、HistoryChain 和 aggregate state/event/presentation 摘要 API 已删除。测试直接比较 typed snapshot 或存档字节，容器 hash 负责保存数据的内容完整性。

## 保存恢复

容器校验 schema、版本、section 唯一性和完整性。`load_with_validation` 在候选 snapshot 上完成宿主 typed 校验，再替换 World；拒绝时保持原会话和失败状态。load 后首 tick 使用 `RestoreContinuation`，之后使用 `Live`。

## 测试

```bash
cargo test -p astra-runtime --test state_machine_tick
cargo test -p astra-runtime --test await_token
cargo test -p astra-runtime --test save_load
cargo test -p astra-runtime --test integrity_mode
cargo test -p astra-runtime --test execution_panic
```

测试覆盖候选 action、事件顺序、完成策略、typed 状态保存、损坏容器拒绝、恢复后的 ID/step 连续性、失败会话恢复，以及两种 observer mode 下 tick 不编码 typed component。产品真实流程与性能的当前完成情况见 [实施状态](../status/implementation-plan.md)。

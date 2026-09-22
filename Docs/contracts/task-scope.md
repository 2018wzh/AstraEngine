# Runtime 异步任务作用域

[重构契约](rebuild.md)规定任务由宿主或 World 拥有，保存显式业务状态。本页对应 `astra_runtime::TaskScope`，不引入新的线程池、registry 或存档格式。

## 生命周期与组合

`TaskScope::run` 驱动返回 `Result<T, E>` 的 future，返回 `TaskOutcome::Completed(T)`、`Failed(E)` 或 `Cancelled`。取消前已完成的返回值保持终态；返回前观察到作用域取消时丢弃结果。父作用域取消会唤醒全部后代，子作用域取消不影响父或兄弟。World 成功读档和销毁会取消旧根，读档预检失败保留旧根。

顺序任务使用一个 async block 中的 `.await?`，后一步只在前一步成功后开始；并行任务使用 `futures_util::try_join!` 等已有组合器，再整体交给 `scope.run`。其中一个分支失败时，组合器丢弃其他未完成分支。作用域不自动 spawn，poll 顺序和线程调度不成为 Runtime 状态权威；完成数据仍通过已有 `AwaitCompletionHandle` 在 tick 边界提交。

`cancelled().await` 供需要完成清理后再退出的工作等待取消通知。`run` 取消会 drop 被包裹 future，因此仅用于允许 drop 取消的工作。已经发出的平台 open、外部进程和 blocking worker 必须由其资源所有者完成关闭或 join，不能靠丢弃 future 声称已撤销外部副作用。单独 drop `run` 不取消同作用域的其他任务，根所有者仍负责退出时 cancel。

## 数据、权限与迁移

作用域、waker、future、闭包和 `TaskOutcome` 均不序列化，不进入 save/package/ABI。业务恢复后从显式保存状态创建新任务。任务不获得额外文件、网络或 World 修改权限；`E` 由调用方持有和处置，不自动写日志或商业 payload。旧 `TaskScope` 和 `AwaitCompletionHandle` API 保留语义，新增等待与 typed outcome；内部取消实现复用 lockfile 中的 `tokio-util`，无自定义通知队列。

## 验证

普通 Runtime 测试覆盖顺序短路、并行成功/失败/取消、未启动就取消、父子与兄弟隔离、丢弃 future 后的资源释放，以及成功/拒绝读档和 World 退出。发布前执行 Engine workspace 的 fmt/clippy/build/test；这些逻辑测试不能替代真实平台异步资源和产品长流程验证。

## 可恢复任务组合

`TaskGroupState` 保存局部任务的 Sequence、All 或 Race 进度；`TaskGroup` 为运行中的成员分配现有 `TaskScope` 子作用域。它不创建线程池，也不持有 worker。Sequence 只激活当前一步，成功后激活下一步，失败或取消终止后续步骤。All 等待每个成员到达终态，结果优先级为 Failed、Cancelled、Completed。Race 接受第一个终态（包括失败和取消），取消其余成员；重复完成、已取消句柄和恢复前的句柄均被拒绝。

组取消不取消父作用域，后继 AwaitToken 可绑定父作用域继续运行。关闭由任务所有者先取消再 join；丢弃组合不能代替 worker 回收。snapshot 记录已完成成员与当前步骤，restore 校验状态并创建新作用域，不启动外部 IO。恢复调用方只恢复显式业务状态，不能重新执行已完成步骤或重放按键。

VN coordinator 是实际消费者：共用 fence 使用 All；文字使用“揭示、独立确认”的 Sequence，其中揭示由时钟与点击 Race 决定。第一次点击只完成未揭示文字，下一次独立输入才能推进剧情。保存包括可见字数、剩余计时与组合进度。它们不保存脚本栈、回调或任务句柄。

[任务组合回归](../../Engine/Source/Runtime/astra-runtime/tests/task_group.rs)覆盖短路、全终态、竞态败者取消、重复结果、恢复代次、父子隔离和 worker 回收。产品数据迁移见[演出帧推进迁移](presentation-tick-migration.md)。真实设备演出仍须另行验证。

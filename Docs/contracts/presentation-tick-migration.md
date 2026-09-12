# 演出帧推进迁移

本次变更落实[重构契约](rebuild.md)中的帧内状态所有权规则，适用于 `astra-vn-presentation` 的 `ProductStageDirector` 和 `PresentationCoordinator`。不新增通用任务引擎，不改变宿主权限或允许演出直接修改剧情 cursor。

## 执行和错误边界

`tick` 在原有 session 中推进 tween、timeline、文字、背景和视频时间，不克隆整个 director/coordinator，也不克隆每条活动 timeline 的完整 keyframe 列表。活动 timeline 暂时移出集合，由同一个 owner 采样并归还未完成的任务。

零 delta、超过一秒的 delta，以及 stage frame/time counter 溢出，在推进前拒绝；对象仍可使用。命令批次继续在提交边界准备 next state，失败不提交任何部分。排队 Show 的 layer/opacity、Movie 的 layer/alpha 和 timeline 的目标、属性、keyframe 范围也在进入队列前验证。`configure_text` 只有提交成功才消费 region sequence。

已经开始执行的 tick 若遇到目标消失、队列激活容量耗尽或内部状态不一致，返回原始 blocking diagnostic，并将该演出 session 标记为 failed。已经推进的帧不回滚。`is_failed()` 对外可查询；后续 tick、命令和 snapshot 返回 `ASTRA_VN_STAGE_SESSION_FAILED` 或 `ASTRA_VN_PRESENTATION_SESSION_FAILED`。无 Result 的文字请求和 activation drain 不再产生新操作。调用宿主须结束受影响的 session，不得忽略错误继续播放。其他 session 不受影响；恢复只能使用此前成功保存的 snapshot 或创建新 session，不能保存失败帧覆盖既有存档。

`prepare_batch` 和 resize 仍是显式边界事务。这里保留的 staging 不能重新移入每帧 tick。普通输入拒绝与执行失败分别有测试，不再用“每次 tick 全部回滚”作为错误恢复承诺。

## 并行轨道和完成

Timeline 的 `ReplaceTarget` 按 target/property 替换冲突轨道；同一目标的其他属性及其他 timeline 继续。camera 的 `main`、`camera`、`camera.main` 别名归一化后判断冲突。取消只移除指定 timeline，不伪造其完成通知。自然结束仍只返回一次 completion fence。

Region 队列保持原顺序并在活动过渡完成后激活；排队状态可存读档。背景增加 `transition_pending`，完成时只提交一次 incoming（包括显式清空），后续 tick 不擦除已显示背景。手动完成文字 reveal 后，下一帧不会把可见字数退回计时器计算值。

## 数据格式迁移

当前 Stage snapshot schema 为 `astra.vn.product_stage_state.v8`，coordinator 为 `astra.vn.presentation_coordinator.v5`。此前 v8/v4 布局已增加内部 failed 字段和背景 pending 标记；本轮 v5 增加等待组一致性校验，字段布局不变。`PresentationRegionCommand` 改用 serde 外部标记枚举，使非空队列可由 postcard 双向编码；旧内部标记表示只能写入，读取会失败。若使用 JSON，payload 形式相应改为如 `{"character": {...}}`，不再是 `{"region": "character", "command": {...}}`。

旧内部 snapshot 明确拒绝并重建，不提供迁移器。恢复时校验 coordinator schema、队列边界、区域匹配、排队策略和文字 reveal rate，拒绝损坏队列而不是延迟到 tick panic。商业游戏原生存档不属于此格式，不能覆盖。

## 验证边界

[Stage 回归](../../Engine/Source/Modules/AstraVN/astra-vn-presentation/tests/support/stage_tick.rs)覆盖 queued Move 的存读档、畸形排队命令拒绝、执行失败终止、从先前存档恢复、同目标不同属性继续、替换和取消。[Coordinator 回归](../../Engine/Source/Modules/AstraVN/astra-vn-presentation/tests/support/coordinator_tick.rs)覆盖多个 region 同时推进、一次性激活和 fence、显式清空及 reveal 不倒退。内部测试补充 counter 溢出和有界 activation 队列耗尽。

本项只证明普通 Rust 演出状态推进和保存边界。实际 Player 视听、性能预算和四平台播放仍须产品运行验证，不由本项单元测试关闭。

## 演出等待组

共用 fence id 的 Character/Background/Text/Video 命令组成 all-of 等待组，包含区域队列中的成员；同组未完成成员的 command id 必须唯一，冲突在批次提交前拒绝。只有所有成员都完成，coordinator 才发出一次完成通知；文字立即显示或视频先结束不能提前放行其他成员。任一成员失败或被新命令替换，组保持 Failed，后续成员完成不能覆盖失败；其他轨道继续执行。已经终结且没有活动成员的 fence id 可以用于新一组命令，重新进入 Pending。

成员身份直接来自现有活动/排队命令，保存同一 coordinator state，不另建线程池或任务 registry。新 coordinator schema 为 v5，旧 v4 快照拒绝重建；StageDirector 外层仍为 v8，恢复时校验内层 schema 与 fence 引用。跨区域并行、顺序排队、文字点击、视频完成/失败、替换与中途保存恢复均需要普通产品状态测试；通用 Runtime 任务组合和产品异步 IO 接入仍未完成。

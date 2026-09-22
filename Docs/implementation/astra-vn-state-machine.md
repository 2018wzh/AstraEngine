# AstraVN 会话执行

当前产品入口是 `astra-vn::VnSession`，契约见 [全产品重构](../contracts/rebuild.md)，模块分工见 [AstraVN](../modules/astra-vn.md)。

VnSession 长期持有 VnRuntime、共享 CompiledStory 与索引。step 先通过 EngineSession 校验逻辑步，再在现有 VN 状态上执行 typed 命令，将 wait 绑定到 Runtime await。它不创建纯转发 StateMachine，也不经过动态 runtime provider。

EngineSession 独立拥有 RuntimeWorld 和任务生命周期。Actor/Component 与可选 flat FSM 继续可用；VN 路线、call/return、系统页和变量属于 VnRuntimeState。保存只在边界物化 VN 状态，普通帧不克隆完整历史。恢复失败保留原状态，成功后使旧作用域结果失效。测试覆盖无 package 嵌入、恢复后继续、关闭隔离、损坏保存和历史分配稳定。

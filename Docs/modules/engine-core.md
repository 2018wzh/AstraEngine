# Engine 核心与开发工具

Engine 是可独立嵌入的 2D 核心，不要求创建会话时携带 package，不强制产品变更都经过 FSM action。公共 `EngineSession` 持有 RuntimeWorld、任务作用域和逻辑步生命周期；`VnSession` 持有 VN 业务状态。当前边界见 [重构契约](../contracts/rebuild.md)。

## 实际 crate 边界

| Crate | 职责 |
| --- | --- |
| `astra-core` | 稳定 ID、诊断、schema、hash 与 PropertySystem metadata |
| `astra-runtime` | EngineSession/RuntimeWorld、Actor/Component、flat FSM、任务、事件、等待与显式保存恢复 |
| `astra-plugin` / `astra-plugin-abi` | 机制插件的 descriptor、loader、注册与实际动态 ABI |
| `astra-target` | Editor/Game/Program target manifest 与验证 |
| `astra-headless-test` | 需要宿主时使用的独占测试上下文，不强制普通逻辑测试启动 Headless |
| `astra-headless` | 物理 JSONL 输入、执行、checkpoint 自动比较与 PNG/WAV/run report |
| `astra-cli` | Cook/package/target/platform/script/UI 与 ReleaseReport 解释 |

AstraEMU Family/Host 不使用 Engine registry、RuntimeWorld 或产品 package。共享文字、绘制、媒体和字节源可独立消费，不因工具调用而新增 VN 或会话依赖。

## 运行与验证

默认逻辑 60 Hz，呈现独立。任务明确完成、取消和失败；读档、重开或退出后旧异步结果失效。保存只在边界编码显式数据，读取失败保留原存档与当前状态；不建设通用输入回放、逐位确定性或每帧全量克隆/序列化。

普通行为使用 Rust unit/integration tests；视听路径实际运行硬件 GPU/音频宿主。自动比较、接口存在或局部 fixture 不代表产品、路线、设备声音或性能验收。

旧 `astra test run`、场景 hash 报告兜底与具名审批链已删除。`astra report explain` 只解释 `astra.release_report.v1`，拒绝其他 schema；实际命令见 [操作手册](../manual/operator-guide.md)，验证结果见 [实施状态](../status/implementation-plan.md)。

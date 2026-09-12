# Headless 测试后端调整

本页按 [全产品重构契约](../contracts/rebuild.md) 更新。原 Migration 11 的强制测试生命周期、状态矩阵和具名审批规则已撤销，历史记录保留在 Git。当前进度见 [实施状态](../status/implementation-plan.md)。

## 按测试需要创建宿主

parser、schema、容器、调度和其他纯逻辑测试使用普通 Rust test 或 Tokio test。测试宏 crate 已删除，不再为了运行数据断言而创建 Headless session。

需要真实视听产物或运行 Headless CLI 的测试显式创建 `HeadlessTestContext`，并持有到所有依赖它的进程和文件读取结束。上下文负责启动和关闭本 worktree 的测试宿主；构建身份文件与临时产物的生命周期随上下文结束。实际执行 CLI 的 fixture 不能只移除旧宏而遗漏这项依赖。

完整产品测试先构建自己的宿主程序。定向普通测试无需预构建宿主，也不接受其他 worktree 的 binary 或 target。命令与空间控制见 [开发与测试](../manual/development.md)。

## 保留实际行为验证

保留需要 Headless 的输入、PNG/WAV 捕获、媒体流、存档恢复、错误和资源关闭测试。测试使用可控输入与明确断言；不能用空窗口、静态报告、首帧或颜色变化代替完整产品行为。

图像与音频比较帮助定位回归，开发 Agent 可检查实际截图和音频分析结果。没有运行设备或资源时明确记录未验证；Headless 通过不能替代真实平台输入、设备音频、GPU 性能或商业游戏长流程。

## 尚待简化的实现

现有协议和 CLI 仍有旧 report、review、tolerance approval 和 preflight 类型及相应调用方。这些是待迁移的实现，不是本轮开发的新审批要求。后续按消费者一起删除，保留实际捕获、比较、损坏输入诊断和生命周期测试，不增加兼容双轨。

Headless 是开发测试宿主，不作为发布平台。发布范围为 Windows、Linux、macOS 和 Android；各平台验证必须记录实际运行结果。

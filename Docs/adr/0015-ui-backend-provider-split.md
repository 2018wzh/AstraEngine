# ADR 0015: 产品 UI 分工

## Context

游戏 UI、作者工作区和 EMU 资料库的交互与生命周期不同，不强制采用同一 widget runtime。当前决定以 [总体架构](../product/architecture.md) 和 [重构契约](../contracts/rebuild.md) 为准。

## Decision

- NativeVN 游戏 UI 使用编译期组合的 Yakui；`.astra` 声明 View/Binding，Rust 提供 typed ViewModel，Luau Controller 保存显式数据。
- Editor 使用 GPUI/gpui-component，共享 `AuthoringWorkspace`；独立 GPU Player 预览复用真实 VN session，不把 Runtime UI 当作 Editor shell。
- EMU Manager 使用 Slint；Family 自持渲染并输出最终帧和 PCM，Manager 负责窗口输出及输入映射，不要求核心复用 Engine/VN UI 或同一 GPU device。
- 删除内部动态 gameplay/UI ABI，不新增后端选择协议或序列化转发层。机制插件与 Family ABI 的真实消费者保持各自边界。

## Consequences

NativeVN 继续保留真实文字 shaping、字体 fallback、Scene2D 绘制和资源生命周期，不能以固定矩形或首帧替代。Editor 的图和时间线修改同一源码；EMU Manager 不拥有原生 VM、媒体或存档权威。实现入口见 [UI Contract](../contracts/ui.md)、[Editor 手册](../../Editor/README.md) 和 [EMU 接口](../contracts/astraemu-ipc.md)。

Slint 许可归因保留在实际运行包的 notices/About，不因产品采用不同 UI 而改变仓库许可证。设备、桌面交互和性能状态只在 [实施状态](../status/implementation-plan.md) 维护，不再沿用旧 Stage 或强制 Headless 审批。
